use clap::{Args, Parser, Subcommand};
use flytunnel_lib::{
    event_sink::{AppEventSink, SharedEventSink},
    frpc_resolver,
    models::{
        FrpcDownloadPayload, TunnelLogPayload, TunnelSettings, TunnelStatusKind,
        TunnelStatusPayload,
    },
    process_manager::TunnelController,
    settings,
};
use std::{
    io::{self, Write},
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "flytunnel-cli",
    about = "Terminal-only Minecraft LAN tunneling powered by frp",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start(StartCommand),
    Probe(BinaryCommand),
    EnsureFrpc(BinaryCommand),
    Paths,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    Show(ConfigShowCommand),
    Set(ConfigSetCommand),
}

#[derive(Args, Debug, Clone, Default)]
struct SettingsOverrides {
    #[arg(long = "server-addr")]
    server_addr: Option<String>,
    #[arg(long = "server-port")]
    server_port: Option<u16>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long = "local-port")]
    local_port: Option<u16>,
    #[arg(long = "remote-port")]
    remote_port: Option<u16>,
    #[arg(long = "frpc-path-override")]
    frpc_path_override: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
struct BinaryCommand {
    #[arg(long = "frpc-path-override")]
    frpc_path_override: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
struct StartCommand {
    #[command(flatten)]
    overrides: SettingsOverrides,
    #[arg(long)]
    save: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct ConfigShowCommand {
    #[arg(long = "show-token")]
    show_token: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct ConfigSetCommand {
    #[command(flatten)]
    overrides: SettingsOverrides,
    #[arg(long = "clear-frpc-path-override")]
    clear_frpc_path_override: bool,
}

struct ConsoleEventSink {
    join_address: Option<String>,
    running_announced: Mutex<bool>,
    latest_error_detail: Mutex<Option<String>>,
}

impl ConsoleEventSink {
    fn shared(join_address: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            join_address,
            running_announced: Mutex::new(false),
            latest_error_detail: Mutex::new(None),
        })
    }

    fn latest_error_detail(&self) -> Option<String> {
        self.latest_error_detail
            .lock()
            .ok()
            .and_then(|detail| detail.clone())
    }
}

impl AppEventSink for ConsoleEventSink {
    fn emit_status(&self, payload: TunnelStatusPayload) {
        let detail = payload
            .detail
            .as_deref()
            .map(strip_ansi)
            .filter(|value| !value.is_empty());

        if payload.status == TunnelStatusKind::Error.as_str() {
            if let Ok(mut latest_error_detail) = self.latest_error_detail.lock() {
                *latest_error_detail = detail.clone();
            }
        }

        match detail.as_deref() {
            Some(detail) if !detail.is_empty() => {
                println!("[status] {} - {}", payload.status, detail)
            }
            _ => println!("[status] {}", payload.status),
        }

        if payload.status == TunnelStatusKind::Running.as_str() {
            if let Ok(mut running_announced) = self.running_announced.lock() {
                if !*running_announced {
                    if let Some(join_address) = &self.join_address {
                        println!("[join] {}", join_address);
                    }
                    *running_announced = true;
                }
            }
        }
    }

    fn emit_log(&self, payload: TunnelLogPayload) {
        let message = strip_ansi(&payload.message);
        match payload.level.as_str() {
            "error" | "warn" => {
                eprintln!("[{}] {}", payload.level, message);
            }
            _ => {
                println!("[{}] {}", payload.level, message);
            }
        }
    }

    fn emit_download(&self, payload: FrpcDownloadPayload) {
        println!("[frpc:{}] {}", payload.stage, payload.message);
        if let Some(path) = payload.path {
            println!("[frpc:path] {}", path);
        }
    }
}

struct ControllerGuard(TunnelController);

impl ControllerGuard {
    fn new(controller: TunnelController) -> Self {
        Self(controller)
    }
}

impl Drop for ControllerGuard {
    fn drop(&mut self) {
        self.0.cleanup();
    }
}

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(io::stderr(), "Error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start(command) => run_start(command),
        Commands::Probe(command) => run_probe(command),
        Commands::EnsureFrpc(command) => run_ensure_frpc(command),
        Commands::Paths => run_paths(),
        Commands::Config { command } => run_config(command),
    }
}

fn run_start(command: StartCommand) -> Result<(), String> {
    let settings = apply_overrides(settings::load_settings()?, &command.overrides, false);
    settings.validate_for_start()?;

    if command.save {
        let saved = settings::save_settings(&settings)?;
        println!(
            "Saved settings to {}",
            settings::settings_path()?.to_string_lossy()
        );
        print_settings(&saved, false);
    }

    println!("Server: {}:{}", settings.server_addr, settings.server_port);
    println!(
        "Tunnel: 127.0.0.1:{} -> {}:{}",
        settings.local_port, settings.server_addr, settings.remote_port
    );
    println!("Press Ctrl+C to stop.");

    let join_address = format!("{}:{}", settings.server_addr, settings.remote_port);
    let console_sink = ConsoleEventSink::shared(Some(join_address));
    let sink: SharedEventSink = console_sink.clone();
    let controller = TunnelController::default();
    let _guard = ControllerGuard::new(controller.clone());
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_flag = stop_requested.clone();

    ctrlc::set_handler(move || {
        stop_flag.store(true, Ordering::SeqCst);
    })
    .map_err(|error| format!("Failed to install Ctrl+C handler: {error}"))?;

    controller.start(sink.clone(), settings)?;
    wait_for_tunnel(&controller, sink, &console_sink, stop_requested)
}

fn run_probe(command: BinaryCommand) -> Result<(), String> {
    let saved = settings::load_settings()?;
    let override_path = resolve_override_path(&saved, command.frpc_path_override);
    let probe = frpc_resolver::probe_frpc(override_path.as_deref())?;

    println!("frpc ready: {}", yes_no(probe.ready));
    println!("version: {}", probe.version);
    println!("source: {}", probe.source.unwrap_or_else(|| "none".into()));
    println!("path: {}", probe.path.unwrap_or_else(|| "<not available>".into()));
    println!("message: {}", probe.display_message);
    Ok(())
}

fn run_ensure_frpc(command: BinaryCommand) -> Result<(), String> {
    let saved = settings::load_settings()?;
    let override_path = resolve_override_path(&saved, command.frpc_path_override);
    let resolution =
        frpc_resolver::ensure_frpc(ConsoleEventSink::shared(None), override_path.as_deref())?;

    println!("frpc ready: {}", yes_no(resolution.ready));
    println!(
        "path: {}",
        resolution.path.unwrap_or_else(|| "<not available>".into())
    );
    Ok(())
}

fn run_paths() -> Result<(), String> {
    println!("settings: {}", settings::settings_path()?.to_string_lossy());
    println!("runtime: {}", settings::runtime_root()?.to_string_lossy());
    println!("bin: {}", settings::bin_root()?.to_string_lossy());
    Ok(())
}

fn run_config(command: ConfigCommand) -> Result<(), String> {
    match command {
        ConfigCommand::Show(command) => run_config_show(command),
        ConfigCommand::Set(command) => run_config_set(command),
    }
}

fn run_config_show(command: ConfigShowCommand) -> Result<(), String> {
    let settings = settings::load_settings()?;
    println!("settings: {}", settings::settings_path()?.to_string_lossy());
    print_settings(&settings, command.show_token);
    Ok(())
}

fn run_config_set(command: ConfigSetCommand) -> Result<(), String> {
    if !has_overrides(&command.overrides) && !command.clear_frpc_path_override {
        return Err("No config changes were provided.".into());
    }

    let merged = apply_overrides(
        settings::load_settings()?,
        &command.overrides,
        command.clear_frpc_path_override,
    );
    let saved = settings::save_settings(&merged)?;

    println!(
        "Saved settings to {}",
        settings::settings_path()?.to_string_lossy()
    );
    print_settings(&saved, false);
    Ok(())
}

fn wait_for_tunnel(
    controller: &TunnelController,
    sink: SharedEventSink,
    console_sink: &Arc<ConsoleEventSink>,
    stop_requested: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut stop_sent = false;

    loop {
        if stop_requested.load(Ordering::SeqCst) && !stop_sent {
            println!("Stopping tunnel...");
            controller.stop(sink.clone())?;
            stop_sent = true;
        }

        let snapshot = controller.snapshot()?;
        if !snapshot.has_child {
            return match snapshot.status {
                TunnelStatusKind::Stopped => Ok(()),
                TunnelStatusKind::Error => Err(console_sink
                    .latest_error_detail()
                    .or(snapshot.last_error)
                    .unwrap_or_else(|| "Tunnel exited with an unknown error.".into())),
                TunnelStatusKind::Starting | TunnelStatusKind::Running => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn print_settings(settings: &TunnelSettings, show_token: bool) {
    let sanitized = settings.sanitized();
    let token_display = if show_token {
        empty_fallback(&sanitized.token, "<not set>").to_string()
    } else {
        mask_token(&sanitized.token)
    };

    println!(
        "server_addr: {}",
        empty_fallback(&sanitized.server_addr, "<not set>")
    );
    println!("server_port: {}", sanitized.server_port);
    println!("token: {}", token_display);
    println!("local_port: {}", sanitized.local_port);
    println!("remote_port: {}", sanitized.remote_port);
    println!(
        "frpc_path_override: {}",
        sanitized
            .frpc_path_override
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("<auto>")
    );
}

fn empty_fallback<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn resolve_override_path(
    settings: &TunnelSettings,
    explicit_override: Option<String>,
) -> Option<String> {
    explicit_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| settings.frpc_path_override.clone())
}

fn has_overrides(overrides: &SettingsOverrides) -> bool {
    overrides.server_addr.is_some()
        || overrides.server_port.is_some()
        || overrides.token.is_some()
        || overrides.local_port.is_some()
        || overrides.remote_port.is_some()
        || overrides.frpc_path_override.is_some()
}

fn apply_overrides(
    mut settings: TunnelSettings,
    overrides: &SettingsOverrides,
    clear_frpc_path_override: bool,
) -> TunnelSettings {
    if let Some(server_addr) = &overrides.server_addr {
        settings.server_addr = server_addr.clone();
    }
    if let Some(server_port) = overrides.server_port {
        settings.server_port = server_port;
    }
    if let Some(token) = &overrides.token {
        settings.token = token.clone();
    }
    if let Some(local_port) = overrides.local_port {
        settings.local_port = local_port;
    }
    if let Some(remote_port) = overrides.remote_port {
        settings.remote_port = remote_port;
    }
    if clear_frpc_path_override {
        settings.frpc_path_override = None;
    } else if let Some(frpc_path_override) = &overrides.frpc_path_override {
        settings.frpc_path_override = Some(frpc_path_override.clone());
    }

    settings.sanitized()
}

fn mask_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return "<not set>".into();
    }
    if trimmed.len() <= 4 {
        return "*".repeat(trimmed.len());
    }

    let tail = &trimmed[trimmed.len() - 4..];
    format!("{}{}", "*".repeat(trimmed.len() - 4), tail)
}

fn strip_ansi(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
        }

        cleaned.push(ch);
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{apply_overrides, has_overrides, mask_token, strip_ansi, SettingsOverrides};
    use flytunnel_lib::models::TunnelSettings;

    #[test]
    fn applies_partial_overrides_without_touching_other_fields() {
        let base = TunnelSettings {
            server_addr: "1.2.3.4".into(),
            server_port: 7000,
            token: "secret-token".into(),
            local_port: 25565,
            remote_port: 25565,
            frpc_path_override: None,
        };
        let overrides = SettingsOverrides {
            server_addr: Some("mc.example.com".into()),
            remote_port: Some(25570),
            ..SettingsOverrides::default()
        };

        let merged = apply_overrides(base, &overrides, false);

        assert_eq!(merged.server_addr, "mc.example.com");
        assert_eq!(merged.server_port, 7000);
        assert_eq!(merged.remote_port, 25570);
        assert_eq!(merged.local_port, 25565);
    }

    #[test]
    fn clear_frpc_path_override_removes_saved_override() {
        let base = TunnelSettings {
            frpc_path_override: Some("C:/tools/frpc.exe".into()),
            ..TunnelSettings::default()
        };

        let merged = apply_overrides(base, &SettingsOverrides::default(), true);

        assert!(merged.frpc_path_override.is_none());
    }

    #[test]
    fn detects_when_no_overrides_are_present() {
        assert!(!has_overrides(&SettingsOverrides::default()));
    }

    #[test]
    fn masks_token_but_keeps_last_four_characters() {
        assert_eq!(mask_token("abcdefghijkl"), "********ijkl");
        assert_eq!(mask_token("abcd"), "****");
        assert_eq!(mask_token(""), "<not set>");
    }

    #[test]
    fn strips_simple_ansi_sequences() {
        assert_eq!(
            strip_ansi("\u{1b}[1;34mhello\u{1b}[0m world"),
            "hello world"
        );
    }
}
