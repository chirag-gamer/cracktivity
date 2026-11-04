pub mod accounts;
pub mod ai;
pub mod app;
pub mod classify;
pub mod mail;
pub mod mcp;
pub mod platform;
pub mod render;
pub mod store;

use tauri::Manager;

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Data dirs per D-006: the encrypted mail DB never roams.
fn profile_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("no local data dir: {e}"))?;
    Ok(base.join("profile"))
}

pub fn run() {
    let e2e_seed = std::env::args().any(|a| a == "--e2e-seed");

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["ctrl+shift+space"])
                .expect("default hotkey parses")
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        platform::shell::toggle_panel(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // Panel before state so the very first hotkey press has a target.
            platform::shell::create_panel(app.handle())?;
            platform::shell::create_tray(app.handle())?;

            let dir = profile_dir(app.handle())?;
            let (state, driver) = app::AppState::new(&dir)?;
            if e2e_seed {
                let st = state.store_ref().lock().map_err(|_| "store poisoned")?;
                app::seed_demo(&st);
            }
            app.manage(state);

            let handle = app.handle().clone();
            std::thread::spawn(move || app::driver_loop(driver, Some(handle)));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close on the main window exits the app fully (FR-31 Quit AC);
            // the panel window only ever hides.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            app::list_accounts,
            app::account_statuses,
            app::thread_list,
            app::thread_messages,
            app::message_body,
            app::search_messages,
            app::unread_total,
            app::mark_read,
            app::set_starred,
            app::archive_message,
            app::set_importance,
            app::add_account,
            app::sync_now,
            app::hide_panel,
        ])
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
