mod ipc;

use ipc::AppInfo;

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo::current()
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_info])
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_crosses_the_ipc_handler() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![get_app_info])
            .build(tauri::generate_context!())
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        tauri::test::assert_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: "get_app_info".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::default(),
                headers: tauri::http::HeaderMap::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
            Ok(serde_json::json!({
                "name": "Lyra",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
            })),
        );
    }
}
