//! Notch dropdown panel shell (D-011, FR-28): borderless top-center window,
//! tray toggle, global hotkey. The recipe is Windows-hardened:
//! - `WS_EX_TOOLWINDOW` is re-asserted after every `show()` because tao
//!   rewrites extended styles on show, wiping externally-set bits (verified
//!   pitfall — panel would otherwise appear in Alt-Tab).
//! - Animation is CSS-only (`translateY` inside a fixed-size window); the
//!   Win32 window itself is never animated (broken in 3 tested ways).
//! - `no_redirection_bitmap` (tauri ≥ 2.12) removes the transparent-window
//!   white flash on show.
//!
//! v1 deviation from D-011 (recorded in decisions): the panel hides fully
//! instead of resting as a clickable notch strip, so `set_ignore_cursor_events`
//! is not needed yet.

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

pub const PANEL_LABEL: &str = "panel";
pub const PANEL_LOGICAL_W: f64 = 480.0;
pub const PANEL_LOGICAL_H: f64 = 640.0;

/// Blur-hide debounce: focus may bounce panel → main within this window
/// (D-011: 150 ms, excluding our own windows).
const BLUR_DEBOUNCE_MS: u64 = 150;

#[cfg(windows)]
fn assert_toolwindow(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(hwnd.0 as _);
    // WS_EX_TOOLWINDOW = 0x80: keeps the panel out of Alt-Tab and the
    // taskbar even though tao resets ex-styles on show().
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | 0x80);
    }
}

#[cfg(not(windows))]
fn assert_toolwindow(_window: &tauri::WebviewWindow) {}

/// Cursor position in physical pixels (which monitor to drop from).
#[cfg(windows)]
fn cursor_pos_physical() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_ok() {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn cursor_pos_physical() -> Option<(i32, i32)> {
    None
}

/// Creates the hidden panel window with the full D-011 flag set.
pub fn create_panel(app: &AppHandle) -> tauri::Result<()> {
    let panel = WebviewWindowBuilder::new(
        app,
        PANEL_LABEL,
        WebviewUrl::App("index.html?window=panel".into()),
    )
    .title("cracktivity panel")
    .inner_size(PANEL_LOGICAL_W, PANEL_LOGICAL_H)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .resizable(false)
    .visible(false)
    .focused(false)
    .no_redirection_bitmap(true)
    .build()?;

    // Blur-hide with debounce: hide only if no cracktivity window has taken
    // focus within the debounce window (D-011 "excluding own windows").
    let app = app.clone();
    panel.on_window_event(move |e| {
        if let WindowEvent::Focused(false) = e {
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(BLUR_DEBOUNCE_MS));
                if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
                    let any_focused = app
                        .webview_windows()
                        .values()
                        .any(|w| w.is_focused().unwrap_or(false));
                    if !any_focused && panel.is_visible().unwrap_or(false) {
                        let _ = panel.hide();
                        let _ = app.emit("panel-visibility", false);
                    }
                }
            });
        }
    });
    Ok(())
}

/// Shows the panel at the top-center of the monitor under the cursor.
/// Call after every open (hotkey, tray, deep-link): re-asserts
/// `WS_EX_TOOLWINDOW` because show() may have reset it (D-011).
pub fn show_panel(app: &AppHandle) -> tauri::Result<()> {
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        return Ok(());
    };
    if panel.is_visible().unwrap_or(false) {
        return Ok(());
    }

    let monitors = app.available_monitors()?;
    let monitor = cursor_pos_physical()
        .and_then(|(x, y)| {
            monitors.iter().find(|m| {
                let pos = m.position();
                let size = m.size();
                x >= pos.x
                    && x < pos.x + size.width as i32
                    && y >= pos.y
                    && y < pos.y + size.height as i32
            })
        })
        .cloned()
        .or_else(|| monitors.into_iter().next());

    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let mpos = monitor.position();
        let msize = monitor.size();
        let panel_w = (PANEL_LOGICAL_W * scale) as i32;
        let x = mpos.x + (msize.width as i32 - panel_w) / 2;
        let y = mpos.y;
        panel.set_position(PhysicalPosition::new(x, y))?;
    }
    assert_toolwindow(&panel);
    panel.show()?;
    panel.set_focus()?;
    let _ = app.emit_to(PANEL_LABEL, "panel-visibility", true);
    Ok(())
}

pub fn hide_panel(app: &AppHandle) -> tauri::Result<()> {
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        panel.hide()?;
        let _ = app.emit("panel-visibility", false);
    }
    Ok(())
}

/// Hotkey/tray toggle (Ctrl+Shift+Space default, FR-28).
pub fn toggle_panel(app: &AppHandle) {
    let visible = app
        .get_webview_window(PANEL_LABEL)
        .and_then(|p| p.is_visible().ok())
        .unwrap_or(false);
    if visible {
        let _ = hide_panel(app);
    } else {
        let _ = show_panel(app);
    }
}

/// Tray icon + menu (FR-31): left-click toggles the panel, right-click opens
/// the menu. Tooltip carries the unread count (badge overlay is deferred).
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let open = MenuItem::with_id(app, "open", "Open cracktivity", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check all accounts", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &check, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }
            "check" => {
                let _ = app.emit("sync-now", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
