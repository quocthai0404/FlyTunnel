use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "FlyTunnel";
pub const FRP_VERSION_TAG: &str = "v0.67.0";
pub const FRP_VERSION_NUMBER: &str = "0.67.0";
pub const MINECRAFT_PROXY_NAME: &str = "minecraft-lan";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelSettings {
    pub server_addr: String,
    pub server_port: u16,
    pub token: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub frpc_path_override: Option<String>,
}

impl Default for TunnelSettings {
    fn default() -> Self {
        Self {
            server_addr: String::new(),
            server_port: 7000,
            token: String::new(),
            local_port: 25565,
            remote_port: 25565,
            frpc_path_override: None,
        }
    }
}

impl TunnelSettings {
    pub fn sanitized(&self) -> Self {
        Self {
            server_addr: self.server_addr.trim().to_string(),
            server_port: self.server_port,
            token: self.token.trim().to_string(),
            local_port: self.local_port,
            remote_port: self.remote_port,
            frpc_path_override: self
                .frpc_path_override
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    pub fn validate_for_start(&self) -> Result<(), String> {
        let sanitized = self.sanitized();

        if sanitized.server_addr.is_empty() {
            return Err("VPS host / IP is required.".into());
        }

        if sanitized.token.is_empty() {
            return Err("Token is required.".into());
        }

        for (label, port) in [
            ("control port", sanitized.server_port),
            ("local port", sanitized.local_port),
            ("remote port", sanitized.remote_port),
        ] {
            if port == 0 {
                return Err(format!("{label} must be between 1 and 65535."));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelStatusKind {
    Stopped,
    Starting,
    Running,
    Error,
}

impl TunnelStatusKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatusPayload {
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TunnelLogPayload {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrpcDownloadPayload {
    pub stage: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrpBinaryStatus {
    pub ready: bool,
    pub path: Option<String>,
    pub source: Option<String>,
    pub version: String,
    pub display_message: String,
}

pub type FrpcResolution = FrpBinaryStatus;
pub type FrpcProbe = FrpBinaryStatus;

#[cfg(test)]
mod tests {
    use super::TunnelSettings;

    #[test]
    fn validates_required_fields() {
        let settings = TunnelSettings::default();
        assert!(settings.validate_for_start().is_err());
    }

    #[test]
    fn accepts_valid_minecraft_defaults() {
        let settings = TunnelSettings {
            server_addr: "127.0.0.1".into(),
            token: "secret".into(),
            ..TunnelSettings::default()
        };

        assert!(settings.validate_for_start().is_ok());
    }
}
