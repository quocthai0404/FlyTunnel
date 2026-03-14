use crate::models::{FrpcDownloadPayload, TunnelLogPayload, TunnelStatusPayload};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub trait AppEventSink: Send + Sync + 'static {
    fn emit_status(&self, payload: TunnelStatusPayload);
    fn emit_log(&self, payload: TunnelLogPayload);
    fn emit_download(&self, payload: FrpcDownloadPayload);
}

pub type SharedEventSink = Arc<dyn AppEventSink>;

pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn shared(app: AppHandle) -> SharedEventSink {
        Arc::new(Self { app })
    }
}

impl AppEventSink for TauriEventSink {
    fn emit_status(&self, payload: TunnelStatusPayload) {
        let _ = self.app.emit("tunnel-status", payload);
    }

    fn emit_log(&self, payload: TunnelLogPayload) {
        let _ = self.app.emit("tunnel-log", payload);
    }

    fn emit_download(&self, payload: FrpcDownloadPayload) {
        let _ = self.app.emit("frpc-download-state", payload);
    }
}

pub struct NoopEventSink;

impl NoopEventSink {
    pub fn shared() -> SharedEventSink {
        Arc::new(Self)
    }
}

impl AppEventSink for NoopEventSink {
    fn emit_status(&self, _payload: TunnelStatusPayload) {}

    fn emit_log(&self, _payload: TunnelLogPayload) {}

    fn emit_download(&self, _payload: FrpcDownloadPayload) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedEvent {
    Status(TunnelStatusPayload),
    Log(TunnelLogPayload),
    Download(FrpcDownloadPayload),
}

#[derive(Default)]
pub struct MemoryEventSink {
    events: Mutex<Vec<RecordedEvent>>,
}

impl MemoryEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn snapshot(&self) -> Vec<RecordedEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl AppEventSink for MemoryEventSink {
    fn emit_status(&self, payload: TunnelStatusPayload) {
        if let Ok(mut events) = self.events.lock() {
            events.push(RecordedEvent::Status(payload));
        }
    }

    fn emit_log(&self, payload: TunnelLogPayload) {
        if let Ok(mut events) = self.events.lock() {
            events.push(RecordedEvent::Log(payload));
        }
    }

    fn emit_download(&self, payload: FrpcDownloadPayload) {
        if let Ok(mut events) = self.events.lock() {
            events.push(RecordedEvent::Download(payload));
        }
    }
}
