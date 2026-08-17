use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy)]
pub enum TooltipState {
    Idle,
    Recording,
    Processing,
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Fuk", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;

    let mut builder = TrayIconBuilder::with_id("dictate")
        .tooltip("Fuk")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        });

    #[cfg(target_os = "macos")]
    {
        // The menu bar wants a template image — pure black plus alpha — which
        // the system recolours for the light or dark bar and for the
        // highlighted state. The full-colour app icon would look like a sticker.
        builder = builder
            .icon(tauri::include_image!("icons/tray.png"))
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
    }

    builder.build(app)?;
    Ok(())
}

pub fn show_settings(app: &AppHandle) {
    show_settings_inner(app);
}

pub fn show_settings_on_main(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || show_settings_inner(&app_main));
}

fn show_settings_inner(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.unminimize();
    }
}

pub fn set_tooltip_state(app: &AppHandle, state: TooltipState) {
    let text = match state {
        TooltipState::Idle => "Fuk",
        TooltipState::Recording => "Fuk — Recording",
        TooltipState::Processing => "Fuk — Processing",
    };
    if let Some(tray) = app.tray_by_id("dictate") {
        let _ = tray.set_tooltip(Some(text));
    }
}
