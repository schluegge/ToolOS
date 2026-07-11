use serde_json::Value;
use toolos_domain::RpcRequest;

#[tauri::command]
async fn daemon_request(method: String, params: Value) -> Result<Value, String> {
    let request = RpcRequest::new(method, params);
    let response = toolos_ipc::request(&request)
        .await
        .map_err(|error| format!("Cannot reach the local ToolOS daemon: {error}"))?;
    if let Some(error) = response.error {
        return Err(format!("Daemon error {}: {}", error.code, error.message));
    }
    Ok(response.result.unwrap_or(Value::Null))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![daemon_request])
        .run(tauri::generate_context!())
        .expect("error while running ToolOS desktop client");
}
