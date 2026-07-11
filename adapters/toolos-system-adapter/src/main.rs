use std::io::{self, BufRead, Write};

use anyhow::Context;
use serde_json::{json, Value};
use toolos_domain::{RpcRequest, RpcResponse, ADAPTER_PROTOCOL_VERSION};

fn main() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line.context("read adapter request")?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => handle_request(request),
            Err(error) => RpcResponse::failure(
                "unknown",
                -32700,
                format!("invalid JSON-RPC request: {error}"),
            ),
        };
        serde_json::to_writer(&mut stdout, &response).context("serialize adapter response")?;
        stdout.write_all(b"\n").context("write adapter newline")?;
        stdout.flush().context("flush adapter response")?;
    }
    Ok(())
}

fn handle_request(request: RpcRequest) -> RpcResponse {
    let result: Result<Value, String> = match request.method.as_str() {
        "adapter.health" => Ok(json!({
            "adapter_id": "toolos.adapter.system",
            "adapter_version": env!("CARGO_PKG_VERSION"),
            "protocol": ADAPTER_PROTOCOL_VERSION,
            "status": "HEALTHY",
            "capabilities": ["machine.inspect", "project.inspect", "archive.inspect"]
        })),
        "machine.inspect" => serde_json::to_value(toolos_system::inspect_machine())
            .map_err(|error| error.to_string()),
        "project.inspect" => request
            .params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "project.inspect requires a string 'path' parameter".to_owned())
            .and_then(|path| {
                serde_json::to_value(toolos_system::inspect_project(path))
                    .map_err(|error| error.to_string())
            }),
        "archive.inspect" => request
            .params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "archive.inspect requires a string 'path' parameter".to_owned())
            .and_then(toolos_archive::inspect_zip)
            .and_then(|report| serde_json::to_value(report).map_err(|error| error.to_string())),
        _ => Err(format!("unknown adapter method: {}", request.method)),
    };

    match result {
        Ok(value) => RpcResponse::success(request.id, value),
        Err(message) => RpcResponse::failure(request.id, -32601, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_exposes_declared_capabilities() {
        let response = handle_request(RpcRequest::new("adapter.health", json!({})));
        let result = response.result.expect("health result");
        assert_eq!(result["status"], "HEALTHY");
        assert_eq!(result["protocol"], ADAPTER_PROTOCOL_VERSION);
        assert!(result["capabilities"]
            .as_array()
            .expect("capability array")
            .contains(&Value::String("archive.inspect".to_owned())));
    }

    #[test]
    fn archive_inspection_requires_path() {
        let response = handle_request(RpcRequest::new("archive.inspect", json!({})));
        assert!(response.result.is_none());
        assert!(response
            .error
            .expect("error")
            .message
            .contains("requires a string 'path'"));
    }

    #[test]
    fn unknown_method_returns_json_rpc_error() {
        let response = handle_request(RpcRequest::new("unknown", json!({})));
        assert!(response.result.is_none());
        assert_eq!(response.error.expect("error").code, -32601);
    }
}
