pub mod event_sink;
pub mod frpc_config;
pub mod frpc_log;
pub mod frpc_resolver;
pub mod models;
pub mod process_manager;
pub mod settings;

use event_sink::TauriEventSink;
use models::{FrpcProbe, FrpcResolution, TunnelSettings};
use process_manager::TunnelController;
use tauri::State;

#[tauri::command]
fn load_settings() -> Result<TunnelSettings, String> {
    settings::load_settings()
}

#[tauri::command]
fn save_settings(settings: TunnelSettings) -> Result<TunnelSettings, String> {
    settings::save_settings(&settings)
}

#[tauri::command]
fn probe_frpc(settings: TunnelSettings) -> Result<FrpcProbe, String> {
    frpc_resolver::probe_frpc(settings.frpc_path_override.as_deref())
}

#[tauri::command]
fn ensure_frpc(app: tauri::AppHandle, settings: TunnelSettings) -> Result<FrpcResolution, String> {
    frpc_resolver::ensure_frpc(
        TauriEventSink::shared(app),
        settings.frpc_path_override.as_deref(),
    )
}

#[tauri::command]
fn start_tunnel(
    app: tauri::AppHandle,
    controller: State<'_, TunnelController>,
    settings: TunnelSettings,
) -> Result<(), String> {
    let saved = settings::save_settings(&settings)?;
    controller.start(TauriEventSink::shared(app), saved)
}

#[tauri::command]
fn stop_tunnel(
    app: tauri::AppHandle,
    controller: State<'_, TunnelController>,
) -> Result<(), String> {
    controller.stop(TauriEventSink::shared(app))
}

#[tauri::command]
fn pick_frpc_binary() -> Option<String> {
    frpc_resolver::pick_frpc_binary()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let controller = TunnelController::default();

    tauri::Builder::default()
        .manage(controller.clone())
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            probe_frpc,
            ensure_frpc,
            start_tunnel,
            stop_tunnel,
            pick_frpc_binary
        ])
        .run(tauri::generate_context!())
        .expect("error while running FlyTunnel");

    controller.cleanup();
}
