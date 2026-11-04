pub mod accounts;
pub mod ai;
pub mod classify;
pub mod mail;
pub mod mcp;
pub mod platform;
pub mod render;
pub mod store;

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .run(tauri::generate_context!())
        .expect("error while running cracktivity");
}

#[cfg(test)]
mod tests {
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;

    /// L2 harness smoke (plan Phase 1 task 3): the command layer answers over
    /// the MockRuntime IPC path, so later commands (store, accounts) are
    /// testable headlessly per D-018.
    #[test]
    fn app_version_command_answers_over_mock_ipc() {
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![super::app_version])
            .build(mock_context(noop_assets()))
            .expect("failed to build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let res = tauri::test::get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "app_version".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<String>().unwrap())
        .expect("ipc response");

        assert_eq!(res, "0.1.0");
    }
}
