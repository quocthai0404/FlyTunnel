use crate::{models::TunnelSettings, settings};
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Serialize)]
struct FrpcConfig {
    #[serde(rename = "serverAddr")]
    server_addr: String,
    #[serde(rename = "serverPort")]
    server_port: u16,
    #[serde(rename = "loginFailExit")]
    login_fail_exit: bool,
    auth: AuthConfig,
    transport: TransportConfig,
    proxies: Vec<ProxyConfig>,
}

#[derive(Serialize)]
struct AuthConfig {
    method: String,
    token: String,
}

#[derive(Serialize)]
struct TransportConfig {
    protocol: String,
}

#[derive(Serialize)]
struct ProxyConfig {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    #[serde(rename = "localIP")]
    local_ip: String,
    #[serde(rename = "localPort")]
    local_port: u16,
    #[serde(rename = "remotePort")]
    remote_port: u16,
}

pub fn render_config(settings: &TunnelSettings) -> Result<String, String> {
    let sanitized = settings.sanitized();
    let config = FrpcConfig {
        server_addr: sanitized.server_addr,
        server_port: sanitized.server_port,
        login_fail_exit: true,
        auth: AuthConfig {
            method: "token".into(),
            token: sanitized.token,
        },
        transport: TransportConfig {
            protocol: "tcp".into(),
        },
        proxies: vec![ProxyConfig {
            name: "minecraft-lan".into(),
            proxy_type: "tcp".into(),
            local_ip: "127.0.0.1".into(),
            local_port: sanitized.local_port,
            remote_port: sanitized.remote_port,
        }],
    };

    toml::to_string_pretty(&config)
        .map_err(|error| format!("Failed to render frpc config: {error}"))
}

pub fn write_runtime_config(settings: &TunnelSettings) -> Result<PathBuf, String> {
    let path = settings::runtime_root()?.join("frpc.toml");
    let rendered = render_config(settings)?;
    fs::write(&path, rendered).map_err(|error| format!("Failed to write frpc config: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::render_config;
    use crate::models::TunnelSettings;

    #[test]
    fn renders_expected_minecraft_toml() {
        let settings = TunnelSettings {
            server_addr: "mc.example.com".into(),
            token: "secret".into(),
            local_port: 25570,
            remote_port: 25580,
            ..TunnelSettings::default()
        };

        let rendered = render_config(&settings).expect("config should render");

        assert!(rendered.contains("serverAddr = \"mc.example.com\""));
        assert!(rendered.contains("token = \"secret\""));
        assert!(rendered.contains("localIP = \"127.0.0.1\""));
        assert!(rendered.contains("localPort = 25570"));
        assert!(rendered.contains("remotePort = 25580"));
    }
}
