use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    GenericFilePath, GenericNamespaced, ListenerOptions,
};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use toolos_domain::{RpcRequest, RpcResponse};

pub const SOCKET_NAMESPACE: &str = "toolos-daemon-v1.sock";

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("local socket error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("daemon closed the connection without a response")]
    EmptyResponse,
}

pub async fn request(request: &RpcRequest) -> Result<RpcResponse, IpcError> {
    let stream = connect_stream().await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut encoded = serde_json::to_vec(request)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    let bytes = reader.read_line(&mut response).await?;
    if bytes == 0 {
        return Err(IpcError::EmptyResponse);
    }
    Ok(serde_json::from_str(response.trim_end())?)
}

pub async fn serve<H, F>(handler: H) -> Result<(), IpcError>
where
    H: Fn(RpcRequest) -> F + Send + Sync + 'static,
    F: Future<Output = RpcResponse> + Send + 'static,
{
    let listener = create_listener()?;
    let handler = Arc::new(handler);
    loop {
        let connection = listener.accept().await?;
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(connection, handler).await {
                tracing::warn!(%error, "local IPC connection failed");
            }
        });
    }
}

async fn handle_connection<H, F>(stream: Stream, handler: Arc<H>) -> Result<(), IpcError>
where
    H: Fn(RpcRequest) -> F + Send + Sync + 'static,
    F: Future<Output = RpcResponse> + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => handler(request).await,
            Err(error) => RpcResponse::failure(
                "unknown",
                -32700,
                format!("invalid JSON-RPC request: {error}"),
            ),
        };
        let mut encoded = serde_json::to_vec(&response)?;
        encoded.push(b'\n');
        writer.write_all(&encoded).await?;
        writer.flush().await?;
    }
    Ok(())
}

async fn connect_stream() -> io::Result<Stream> {
    if GenericNamespaced::is_supported() {
        let name = SOCKET_NAMESPACE.to_ns_name::<GenericNamespaced>()?;
        Stream::connect(name).await
    } else {
        let path = filesystem_socket_path();
        let name = path.to_fs_name::<GenericFilePath>()?;
        Stream::connect(name).await
    }
}

fn create_listener() -> io::Result<interprocess::local_socket::tokio::Listener> {
    if GenericNamespaced::is_supported() {
        let name = SOCKET_NAMESPACE.to_ns_name::<GenericNamespaced>()?;
        ListenerOptions::new().name(name).create_tokio()
    } else {
        let path = filesystem_socket_path();
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let name = path.to_fs_name::<GenericFilePath>()?;
        ListenerOptions::new().name(name).create_tokio()
    }
}

fn filesystem_socket_path() -> PathBuf {
    std::env::temp_dir().join(SOCKET_NAMESPACE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn socket_name_is_versioned() {
        assert!(SOCKET_NAMESPACE.contains("v1"));
    }

    #[tokio::test]
    async fn response_round_trip_serialization() {
        let response = RpcResponse::success("id", json!({"ok": true}));
        let encoded = serde_json::to_string(&response).expect("serialize");
        let decoded: RpcResponse = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, response);
    }
}
