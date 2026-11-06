use std::fs;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;

const MAIN_WINDOW: &str = "main";

/// Marker file whose existence means the streamer has already seen the
/// "still running in the tray" notice.
const TRAY_NOTICE_MARKER: &str = "tray-notice-shown";

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Tells the streamer once that closing the window doesn't stop the app, so a
/// Practice Chat doesn't silently keep running (or silently die) mid-stream.
fn show_tray_notice_once(app: &AppHandle) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let marker = config_dir.join(TRAY_NOTICE_MARKER);
    if marker.exists() {
        return;
    }
    let _ = fs::create_dir_all(&config_dir);
    let _ = fs::write(&marker, "");

    let place = if cfg!(target_os = "macos") { "menu bar" } else { "system tray" };
    app.dialog()
        .message(format!(
            "Small Streamer Chat is still running in the {place}.\n\nUse its icon there to reopen the window or quit."
        ))
        .title("Still running")
        .show(|_| {});
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Small Streamer Chat", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("app has a default icon"))
        .tooltip("Small Streamer Chat")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
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
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be registered first. A second launch focuses the running app
    // instead of starting a second Practice Chat.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }));
    }

    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it; the app keeps running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == MAIN_WINDOW
            {
                api.prevent_close();
                let _ = window.hide();
                show_tray_notice_once(window.app_handle());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // macOS: clicking the Dock icon while the window is hidden reopens it.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
