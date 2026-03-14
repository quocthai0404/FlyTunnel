use crate::{
    event_sink::SharedEventSink,
    frpc_config,
    frpc_log::{classify_frpc_line, is_minecraft_proxy_started, FrpcSignal},
    frpc_resolver,
    models::{TunnelLogPayload, TunnelSettings, TunnelStatusKind, TunnelStatusPayload},
};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[derive(Clone, Default)]
pub struct TunnelController {
    inner: Arc<Mutex<TunnelRuntime>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelSnapshot {
    pub status: TunnelStatusKind,
    pub has_child: bool,
    pub stop_requested: bool,
    pub login_confirmed: bool,
    pub proxy_confirmed: bool,
    pub last_error: Option<String>,
}

struct TunnelRuntime {
    child: Option<Child>,
    config_path: Option<PathBuf>,
    status: TunnelStatusKind,
    stop_requested: bool,
    login_confirmed: bool,
    proxy_confirmed: bool,
    last_error: Option<String>,
}

impl Default for TunnelRuntime {
    fn default() -> Self {
        Self {
            child: None,
            config_path: None,
            status: TunnelStatusKind::Stopped,
            stop_requested: false,
            login_confirmed: false,
            proxy_confirmed: false,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExitOutcome {
    status: TunnelStatusKind,
    detail: String,
    log_level: &'static str,
    log_message: String,
    should_emit: bool,
}

impl TunnelController {
    pub fn start(&self, sink: SharedEventSink, settings: TunnelSettings) -> Result<(), String> {
        settings.validate_for_start()?;

        {
            let mut runtime = self.lock_runtime()?;
            if runtime.child.is_some() {
                return Err("Tunnel is already running.".into());
            }

            runtime.status = TunnelStatusKind::Starting;
            runtime.stop_requested = false;
            runtime.login_confirmed = false;
            runtime.proxy_confirmed = false;
            runtime.last_error = None;
            runtime.config_path = None;
        }

        emit_status(
            &sink,
            TunnelStatusKind::Starting,
            Some("Preparing frpc and opening the tunnel...".into()),
        );

        let start_result: Result<(), String> = (|| -> Result<(), String> {
            let resolution =
                frpc_resolver::ensure_frpc(sink.clone(), settings.frpc_path_override.as_deref())?;
            let resolution_path = resolution
                .path
                .clone()
                .ok_or_else(|| "frpc binary path is unavailable.".to_string())?;
            let config_path = frpc_config::write_runtime_config(&settings)?;
            let mut command = Command::new(resolution_path);
            command.arg("-c").arg(&config_path);
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }

            let mut child = command
                .spawn()
                .map_err(|error| format!("Failed to start frpc: {error}"))?;
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            {
                let mut runtime = self.lock_runtime()?;
                runtime.config_path = Some(config_path);
                runtime.child = Some(child);
                runtime.status = TunnelStatusKind::Starting;
            }

            emit_log(
                &sink,
                "info",
                "frpc launched. Waiting for the tunnel to come online.",
            );

            if let Some(stdout) = stdout {
                self.spawn_stream_reader(sink.clone(), stdout, "info");
            }

            if let Some(stderr) = stderr {
                self.spawn_stream_reader(sink.clone(), stderr, "warn");
            }

            self.spawn_monitor(sink.clone());
            Ok(())
        })();

        if let Err(error) = start_result {
            let config_path = {
                let mut runtime = self.lock_runtime()?;
                runtime.child = None;
                runtime.status = TunnelStatusKind::Error;
                runtime.stop_requested = false;
                runtime.login_confirmed = false;
                runtime.proxy_confirmed = false;
                runtime.last_error = Some(error.clone());
                runtime.config_path.take()
            };
            cleanup_runtime_file(config_path);
            emit_log(&sink, "error", &error);
            emit_status(&sink, TunnelStatusKind::Error, Some(error.clone()));
            return Err(error);
        }

        Ok(())
    }

    pub fn stop(&self, sink: SharedEventSink) -> Result<(), String> {
        let (mut child, config_path) = {
            let mut runtime = self.lock_runtime()?;
            runtime.stop_requested = true;
            runtime.status = TunnelStatusKind::Stopped;
            runtime.login_confirmed = false;
            runtime.proxy_confirmed = false;
            runtime.last_error = None;
            (runtime.child.take(), runtime.config_path.take())
        };

        if let Some(child) = child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }

        cleanup_runtime_file(config_path);
        emit_log(&sink, "info", "Tunnel stopped.");
        emit_status(
            &sink,
            TunnelStatusKind::Stopped,
            Some("Tunnel stopped.".into()),
        );
        Ok(())
    }

    pub fn cleanup(&self) {
        let (mut child, config_path) = {
            let mut runtime = match self.inner.lock() {
                Ok(runtime) => runtime,
                Err(_) => return,
            };
            runtime.stop_requested = true;
            runtime.status = TunnelStatusKind::Stopped;
            runtime.login_confirmed = false;
            runtime.proxy_confirmed = false;
            runtime.last_error = None;
            (runtime.child.take(), runtime.config_path.take())
        };

        if let Some(child) = child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }

        cleanup_runtime_file(config_path);
    }

    pub fn snapshot(&self) -> Result<TunnelSnapshot, String> {
        let runtime = self.lock_runtime()?;

        Ok(TunnelSnapshot {
            status: runtime.status,
            has_child: runtime.child.is_some(),
            stop_requested: runtime.stop_requested,
            login_confirmed: runtime.login_confirmed,
            proxy_confirmed: runtime.proxy_confirmed,
            last_error: runtime.last_error.clone(),
        })
    }

    fn spawn_monitor(&self, sink: SharedEventSink) {
        let controller = self.clone();

        thread::spawn(move || loop {
            let monitor_state = {
                let mut runtime = match controller.inner.lock() {
                    Ok(runtime) => runtime,
                    Err(_) => return,
                };

                let Some(child) = runtime.child.as_mut() else {
                    return;
                };

                match child.try_wait() {
                    Ok(Some(status)) => {
                        let outcome = decide_exit_outcome(
                            runtime.status,
                            runtime.stop_requested,
                            runtime.login_confirmed && runtime.proxy_confirmed,
                            status.success(),
                            status.code(),
                            runtime.last_error.clone(),
                        );
                        let config_path = runtime.config_path.take();
                        runtime.child = None;
                        runtime.status = outcome.status;
                        runtime.stop_requested = false;
                        runtime.login_confirmed = false;
                        runtime.proxy_confirmed = false;
                        runtime.last_error = None;
                        Some((config_path, outcome))
                    }
                    Ok(None) => None,
                    Err(error) => {
                        let detail = format!("Failed to monitor frpc: {error}");
                        let outcome = ExitOutcome {
                            status: TunnelStatusKind::Error,
                            detail: detail.clone(),
                            log_level: "error",
                            log_message: detail,
                            should_emit: true,
                        };
                        let config_path = runtime.config_path.take();
                        runtime.child = None;
                        runtime.status = TunnelStatusKind::Error;
                        runtime.stop_requested = false;
                        runtime.login_confirmed = false;
                        runtime.proxy_confirmed = false;
                        runtime.last_error = None;
                        Some((config_path, outcome))
                    }
                }
            };

            if let Some((config_path, outcome)) = monitor_state {
                cleanup_runtime_file(config_path);

                if outcome.should_emit {
                    emit_log(&sink, outcome.log_level, &outcome.log_message);
                    emit_status(&sink, outcome.status, Some(outcome.detail));
                }

                return;
            }

            thread::sleep(Duration::from_millis(250));
        });
    }

    fn spawn_stream_reader<R>(&self, sink: SharedEventSink, reader: R, level: &'static str)
    where
        R: Read + Send + 'static,
    {
        let controller = self.clone();

        thread::spawn(move || {
            let reader = BufReader::new(reader);

            for line in reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        emit_log(&sink, level, &line);

                        if let Some(signal) = classify_frpc_line(&line) {
                            controller.handle_signal(&sink, signal);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let detail = format!("Failed to read frpc output: {error}");
                        controller.mark_error(&sink, detail.clone());
                        emit_log(&sink, "error", &detail);
                        return;
                    }
                }
            }
        });
    }

    fn handle_signal(&self, sink: &SharedEventSink, signal: FrpcSignal) {
        let mut status_to_emit: Option<(TunnelStatusKind, String)> = None;
        let mut extra_log: Option<(&'static str, String)> = None;

        {
            let mut runtime = match self.lock_runtime() {
                Ok(runtime) => runtime,
                Err(_) => return,
            };

            if runtime.stop_requested {
                return;
            }

            match signal {
                FrpcSignal::Connecting => {
                    if runtime.status != TunnelStatusKind::Error {
                        runtime.status = TunnelStatusKind::Starting;
                        status_to_emit = Some((
                            TunnelStatusKind::Starting,
                            "Connecting to your VPS...".into(),
                        ));
                    }
                }
                FrpcSignal::LoginSuccess => {
                    runtime.login_confirmed = true;
                    if runtime.status != TunnelStatusKind::Error {
                        runtime.status = TunnelStatusKind::Starting;
                        status_to_emit = Some((
                            TunnelStatusKind::Starting,
                            "Authenticated with frps. Opening the Minecraft port...".into(),
                        ));
                    }
                }
                FrpcSignal::ProxyStarted { .. } if is_minecraft_proxy_started(&signal) => {
                    runtime.proxy_confirmed = true;
                }
                FrpcSignal::StartError { detail } | FrpcSignal::LoginFailed { detail } => {
                    runtime.last_error = Some(detail.clone());
                    runtime.status = TunnelStatusKind::Error;
                    status_to_emit = Some((TunnelStatusKind::Error, detail.clone()));
                    extra_log = Some(("error", detail));
                }
                _ => {}
            }

            if runtime.status != TunnelStatusKind::Error
                && runtime.login_confirmed
                && runtime.proxy_confirmed
            {
                runtime.status = TunnelStatusKind::Running;
                status_to_emit = Some((
                    TunnelStatusKind::Running,
                    "Tunnel is live. Share the join address with your friends.".into(),
                ));
                extra_log = Some(("info", "Tunnel started.".into()));
            }
        }

        if let Some((level, message)) = extra_log {
            emit_log(sink, level, &message);
        }

        if let Some((status, detail)) = status_to_emit {
            emit_status(sink, status, Some(detail));
        }
    }

    fn mark_error(&self, sink: &SharedEventSink, detail: String) {
        let should_emit = {
            let mut runtime = match self.lock_runtime() {
                Ok(runtime) => runtime,
                Err(_) => return,
            };

            if runtime.stop_requested {
                return;
            }

            runtime.last_error = Some(detail.clone());
            runtime.status = TunnelStatusKind::Error;
            true
        };

        if should_emit {
            emit_status(sink, TunnelStatusKind::Error, Some(detail));
        }
    }

    fn lock_runtime(&self) -> Result<std::sync::MutexGuard<'_, TunnelRuntime>, String> {
        self.inner
            .lock()
            .map_err(|_| "Tunnel state is unavailable right now.".into())
    }
}

fn decide_exit_outcome(
    status_before_exit: TunnelStatusKind,
    stop_requested: bool,
    ready: bool,
    success: bool,
    exit_code: Option<i32>,
    last_error: Option<String>,
) -> ExitOutcome {
    if stop_requested {
        return ExitOutcome {
            status: TunnelStatusKind::Stopped,
            detail: "Tunnel stopped.".into(),
            log_level: "info",
            log_message: "Tunnel stopped.".into(),
            should_emit: false,
        };
    }

    if status_before_exit == TunnelStatusKind::Error {
        let detail = last_error.unwrap_or_else(|| match exit_code {
            Some(code) => format!("frpc exited with code {code}."),
            None => "frpc exited unexpectedly.".into(),
        });
        return ExitOutcome {
            status: TunnelStatusKind::Error,
            detail: detail.clone(),
            log_level: "error",
            log_message: detail,
            should_emit: false,
        };
    }

    if success && ready {
        return ExitOutcome {
            status: TunnelStatusKind::Stopped,
            detail: "Tunnel stopped.".into(),
            log_level: "info",
            log_message: "Tunnel stopped.".into(),
            should_emit: true,
        };
    }

    let detail = last_error.unwrap_or_else(|| {
        if success {
            "frpc exited before the tunnel became ready.".into()
        } else {
            match exit_code {
                Some(code) => format!("frpc exited with code {code}."),
                None => "frpc exited unexpectedly.".into(),
            }
        }
    });

    ExitOutcome {
        status: TunnelStatusKind::Error,
        detail: detail.clone(),
        log_level: "error",
        log_message: detail,
        should_emit: true,
    }
}

fn emit_status(sink: &SharedEventSink, status: TunnelStatusKind, detail: Option<String>) {
    sink.emit_status(TunnelStatusPayload {
        status: status.as_str().into(),
        detail,
    });
}

fn emit_log(sink: &SharedEventSink, level: &str, message: &str) {
    sink.emit_log(TunnelLogPayload {
        level: level.into(),
        message: message.into(),
    });
}

fn cleanup_runtime_file(path: Option<PathBuf>) {
    if let Some(path) = path {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::decide_exit_outcome;
    use crate::models::TunnelStatusKind;

    #[test]
    fn stop_requested_does_not_emit_false_error() {
        let outcome =
            decide_exit_outcome(TunnelStatusKind::Stopped, true, true, false, Some(1), None);
        assert_eq!(outcome.status, TunnelStatusKind::Stopped);
        assert!(!outcome.should_emit);
    }

    #[test]
    fn premature_exit_becomes_error() {
        let outcome = decide_exit_outcome(
            TunnelStatusKind::Starting,
            false,
            false,
            false,
            Some(1),
            None,
        );
        assert_eq!(outcome.status, TunnelStatusKind::Error);
        assert_eq!(outcome.log_level, "error");
        assert!(outcome.detail.contains("code 1"));
    }
}
