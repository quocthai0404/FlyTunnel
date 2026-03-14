use flytunnel_lib::{
    event_sink::{MemoryEventSink, NoopEventSink, RecordedEvent, SharedEventSink},
    frpc_resolver::{self, FrpBinaryKind},
    models::{TunnelSettings, TunnelStatusKind},
    process_manager::TunnelController,
    settings,
};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn set_test_roots(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join("flytunnel-e2e");
    let config_root = root.join("config").join(name);
    let data_root = root.join("data");
    let _ = fs::create_dir_all(&config_root);
    let _ = fs::create_dir_all(&data_root);
    std::env::set_var("FLYTUNNEL_CONFIG_DIR", &config_root);
    std::env::set_var("FLYTUNNEL_DATA_DIR", &data_root);
    root
}

fn clear_test_roots() {
    std::env::remove_var("FLYTUNNEL_CONFIG_DIR");
    std::env::remove_var("FLYTUNNEL_DATA_DIR");
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("free port listener")
        .local_addr()
        .expect("local addr")
        .port()
}

struct EchoServer {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl EchoServer {
    fn start(port: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("echo listener");
        listener
            .set_nonblocking(true)
            .expect("echo listener should be non-blocking");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0u8; 1024];
                        match stream.read(&mut buffer) {
                            Ok(read) if read > 0 => {
                                let _ = stream.write_all(&buffer[..read]);
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for EchoServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct FrpsProcess {
    child: Child,
}

impl FrpsProcess {
    fn start(control_port: u16, token: &str, name: &str) -> Self {
        let binary =
            frpc_resolver::ensure_host_binary(FrpBinaryKind::Server, NoopEventSink::shared())
                .expect("frps binary should be available");
        let binary_path = binary.path.expect("frps binary path");
        let config_path = std::env::temp_dir()
            .join("flytunnel-e2e")
            .join(format!("frps-{name}-{control_port}.toml"));
        let rendered = format!(
            "bindPort = {control_port}\n\nauth.method = \"token\"\nauth.token = \"{token}\"\nlog.to = \"console\"\nlog.level = \"info\"\n"
        );
        fs::write(&config_path, rendered).expect("frps config should be written");

        let mut command = Command::new(binary_path);
        command.arg("-c").arg(&config_path);
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        let child = command.spawn().expect("frps should start");
        wait_for_port_open(control_port, Duration::from_secs(5))
            .expect("frps control port should open");

        Self { child }
    }
}

impl Drop for FrpsProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ControllerCleanup(TunnelController);

impl ControllerCleanup {
    fn new(controller: TunnelController) -> Self {
        Self(controller)
    }
}

impl Drop for ControllerCleanup {
    fn drop(&mut self) {
        self.0.cleanup();
    }
}

fn wait_for_port_open(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(format!("Timed out waiting for port {port} to open."))
}

fn wait_for_port_closed(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_err() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(format!("Timed out waiting for port {port} to close."))
}

fn wait_for_status(
    sink: &Arc<MemoryEventSink>,
    expected: TunnelStatusKind,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let events = sink.snapshot();
        for event in events.iter().rev() {
            if let RecordedEvent::Status(payload) = event {
                if payload.status == expected.as_str() {
                    return Ok(payload.detail.clone().unwrap_or_default());
                }
                if payload.status == TunnelStatusKind::Error.as_str()
                    && expected != TunnelStatusKind::Error
                {
                    return Err(payload
                        .detail
                        .clone()
                        .unwrap_or_else(|| "Unexpected tunnel error.".into()));
                }
            }
        }

        thread::sleep(Duration::from_millis(80));
    }

    Err(format!("Timed out waiting for {}.", expected.as_str()))
}

fn assert_runtime_config_cleaned() {
    let runtime_config = settings::runtime_root()
        .expect("runtime root")
        .join("frpc.toml");
    assert!(
        !runtime_config.exists(),
        "runtime config should be removed after shutdown"
    );
}

fn roundtrip_remote_payload(remote_port: u16) {
    let mut stream =
        TcpStream::connect(("127.0.0.1", remote_port)).expect("remote port should accept");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("read timeout");
    stream
        .write_all(b"ping-flytunnel")
        .expect("payload should send");
    let mut buffer = [0u8; 64];
    let read = stream.read(&mut buffer).expect("payload should echo");
    assert_eq!(&buffer[..read], b"ping-flytunnel");
}

fn latest_status_detail(sink: &Arc<MemoryEventSink>, status: TunnelStatusKind) -> Option<String> {
    sink.snapshot()
        .into_iter()
        .rev()
        .find_map(|event| match event {
            RecordedEvent::Status(payload) if payload.status == status.as_str() => payload.detail,
            _ => None,
        })
}

fn status_trace(sink: &Arc<MemoryEventSink>) -> Vec<String> {
    sink.snapshot()
        .into_iter()
        .filter_map(|event| match event {
            RecordedEvent::Status(payload) => Some(format!(
                "{}:{}",
                payload.status,
                payload.detail.unwrap_or_default()
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn acceptance_suite_covers_success_failure_and_cleanup() {
    let _guard = env_lock().lock().expect("env lock");
    let _root = set_test_roots("acceptance-suite");
    println!("preparing official frp binaries");
    let _ = frpc_resolver::ensure_host_binary(FrpBinaryKind::Server, NoopEventSink::shared())
        .expect("frps binary should download");
    let _ = frpc_resolver::ensure_frpc(NoopEventSink::shared(), None)
        .expect("frpc binary should download");

    println!("running success-and-stop case");
    run_success_and_stop_case();
    println!("running invalid-token case");
    run_invalid_token_case();
    println!("running unreachable-server case");
    run_unreachable_server_case();
    println!("running remote-port-conflict case");
    run_remote_port_conflict_case();
    println!("running cleanup-on-shutdown case");
    run_cleanup_on_shutdown_case();

    clear_test_roots();
}

fn run_success_and_stop_case() {
    let control_port = free_port();
    let local_port = free_port();
    let remote_port = free_port();
    let token = "flytunnel-e2e";
    let _echo_server = EchoServer::start(local_port);
    let _frps = FrpsProcess::start(control_port, token, "success");
    let controller = TunnelController::default();
    let _cleanup = ControllerCleanup::new(controller.clone());
    let memory_sink = MemoryEventSink::shared();
    let sink: SharedEventSink = memory_sink.clone();

    controller
        .start(
            sink.clone(),
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: token.into(),
                local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("tunnel should start");

    wait_for_status(
        &memory_sink,
        TunnelStatusKind::Running,
        Duration::from_secs(20),
    )
    .unwrap_or_else(|error| panic!("{error}. statuses: {:?}", status_trace(&memory_sink)));
    roundtrip_remote_payload(remote_port);

    controller.stop(sink).expect("stop should succeed");
    wait_for_port_closed(remote_port, Duration::from_secs(6))
        .expect("remote port should close after stop");
    assert_runtime_config_cleaned();
}

fn run_invalid_token_case() {
    let control_port = free_port();
    let local_port = free_port();
    let remote_port = free_port();
    let _echo_server = EchoServer::start(local_port);
    let _frps = FrpsProcess::start(control_port, "correct-token", "invalid-token");
    let controller = TunnelController::default();
    let _cleanup = ControllerCleanup::new(controller.clone());
    let memory_sink = MemoryEventSink::shared();
    let sink: SharedEventSink = memory_sink.clone();

    controller
        .start(
            sink.clone(),
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: "wrong-token".into(),
                local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("frpc process should still launch");

    let detail = wait_for_status(
        &memory_sink,
        TunnelStatusKind::Error,
        Duration::from_secs(20),
    )
    .expect("invalid token should error");
    assert!(
        detail.to_ascii_lowercase().contains("token")
            || detail.to_ascii_lowercase().contains("login"),
        "unexpected invalid-token detail: {detail}"
    );
    controller.cleanup();
    assert_runtime_config_cleaned();
}

fn run_unreachable_server_case() {
    let control_port = free_port();
    let local_port = free_port();
    let remote_port = free_port();
    let controller = TunnelController::default();
    let _cleanup = ControllerCleanup::new(controller.clone());
    let memory_sink = MemoryEventSink::shared();
    let sink: SharedEventSink = memory_sink.clone();

    controller
        .start(
            sink.clone(),
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: "unused".into(),
                local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("frpc process should still launch");

    let detail = wait_for_status(
        &memory_sink,
        TunnelStatusKind::Error,
        Duration::from_secs(20),
    )
    .expect("unreachable server should error");
    let lowered = detail.to_ascii_lowercase();
    assert!(
        lowered.contains("refused")
            || lowered.contains("failed")
            || lowered.contains("timeout")
            || lowered.contains("unable"),
        "unexpected unreachable detail: {detail}"
    );
    controller.cleanup();
    assert_runtime_config_cleaned();
}

fn run_remote_port_conflict_case() {
    let control_port = free_port();
    let primary_local_port = free_port();
    let secondary_local_port = free_port();
    let remote_port = free_port();
    let _primary_echo = EchoServer::start(primary_local_port);
    let _secondary_echo = EchoServer::start(secondary_local_port);
    let _frps = FrpsProcess::start(control_port, "shared-token", "remote-conflict");
    let primary_controller = TunnelController::default();
    let _primary_cleanup = ControllerCleanup::new(primary_controller.clone());
    let primary_sink_store = MemoryEventSink::shared();
    let primary_sink: SharedEventSink = primary_sink_store.clone();

    primary_controller
        .start(
            primary_sink.clone(),
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: "shared-token".into(),
                local_port: primary_local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("frpc process should still launch");

    wait_for_status(
        &primary_sink_store,
        TunnelStatusKind::Running,
        Duration::from_secs(20),
    )
    .unwrap_or_else(|error| panic!("{error}. statuses: {:?}", status_trace(&primary_sink_store)));

    let secondary_controller = TunnelController::default();
    let _secondary_cleanup = ControllerCleanup::new(secondary_controller.clone());
    let secondary_sink_store = MemoryEventSink::shared();
    let secondary_sink: SharedEventSink = secondary_sink_store.clone();

    secondary_controller
        .start(
            secondary_sink.clone(),
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: "shared-token".into(),
                local_port: secondary_local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("second frpc process should still launch");

    let detail = wait_for_status(
        &secondary_sink_store,
        TunnelStatusKind::Error,
        Duration::from_secs(12),
    )
    .unwrap_or_else(|error| {
        panic!(
            "{error}. statuses: {:?}",
            status_trace(&secondary_sink_store)
        )
    });
    let lowered = detail.to_ascii_lowercase();
    assert!(
        lowered.contains("start error") || lowered.contains("port") || lowered.contains("already"),
        "unexpected port-conflict detail: {detail}"
    );
    secondary_controller.cleanup();
    primary_controller.cleanup();
    wait_for_port_closed(remote_port, Duration::from_secs(6))
        .expect("remote port should close after conflict cleanup");
    assert_runtime_config_cleaned();
}

fn run_cleanup_on_shutdown_case() {
    let control_port = free_port();
    let local_port = free_port();
    let remote_port = free_port();
    let token = "cleanup-token";
    let _echo_server = EchoServer::start(local_port);
    let _frps = FrpsProcess::start(control_port, token, "cleanup");
    let controller = TunnelController::default();
    let _cleanup = ControllerCleanup::new(controller.clone());
    let memory_sink = MemoryEventSink::shared();
    let sink: SharedEventSink = memory_sink.clone();

    controller
        .start(
            sink,
            TunnelSettings {
                server_addr: "127.0.0.1".into(),
                server_port: control_port,
                token: token.into(),
                local_port,
                remote_port,
                frpc_path_override: None,
            },
        )
        .expect("tunnel should start");

    wait_for_status(
        &memory_sink,
        TunnelStatusKind::Running,
        Duration::from_secs(20),
    )
    .unwrap_or_else(|error| panic!("{error}. statuses: {:?}", status_trace(&memory_sink)));
    roundtrip_remote_payload(remote_port);

    controller.cleanup();
    wait_for_port_closed(remote_port, Duration::from_secs(6))
        .expect("remote port should close after cleanup");
    let error_detail = latest_status_detail(&memory_sink, TunnelStatusKind::Error);
    assert!(
        error_detail.is_none(),
        "cleanup should not emit a false error, saw: {:?}",
        error_detail
    );
    assert_runtime_config_cleaned();
}
