use crate::models::MINECRAFT_PROXY_NAME;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrpcSignal {
    Connecting,
    LoginSuccess,
    ProxyStarted { proxy_name: String },
    StartError { detail: String },
    LoginFailed { detail: String },
}

pub fn classify_frpc_line(line: &str) -> Option<FrpcSignal> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();

    if lowered.contains("try to connect to server") {
        return Some(FrpcSignal::Connecting);
    }

    if lowered.contains("login to server success") {
        return Some(FrpcSignal::LoginSuccess);
    }

    if let Some((proxy_name, _detail)) = extract_proxy_event(trimmed, "start proxy success") {
        return Some(FrpcSignal::ProxyStarted {
            proxy_name: proxy_name.to_string(),
        });
    }

    if lowered.contains("token in login doesn't match token from configuration") {
        return Some(FrpcSignal::LoginFailed {
            detail: trimmed.to_string(),
        });
    }

    if lowered.contains("login to the server failed")
        || lowered.contains("authorization failed")
        || lowered.contains("authentication failed")
    {
        return Some(FrpcSignal::LoginFailed {
            detail: trimmed.to_string(),
        });
    }

    if let Some((_proxy_name, detail)) = extract_proxy_event(trimmed, "start error") {
        return Some(FrpcSignal::StartError {
            detail: detail.to_string(),
        });
    }

    None
}

fn extract_proxy_event<'a>(line: &'a str, suffix: &str) -> Option<(&'a str, &'a str)> {
    let lowered = line.to_ascii_lowercase();
    let suffix_index = lowered.find(suffix)?;
    let prefix = &line[..suffix_index];
    let start_index = prefix.rfind('[')?;
    let end_index = prefix.get(start_index..)?.find(']')? + start_index;
    let proxy_name = &line[start_index + 1..end_index];
    let remainder = line.get(end_index + 1..)?.trim();

    if !remainder.to_ascii_lowercase().starts_with(suffix) {
        return None;
    }

    Some((proxy_name, remainder))
}

pub fn is_minecraft_proxy_started(signal: &FrpcSignal) -> bool {
    matches!(
        signal,
        FrpcSignal::ProxyStarted { proxy_name } if proxy_name == MINECRAFT_PROXY_NAME
    )
}

#[cfg(test)]
mod tests {
    use super::{classify_frpc_line, is_minecraft_proxy_started, FrpcSignal};

    #[test]
    fn parses_success_transition_lines() {
        assert_eq!(
            classify_frpc_line("[I] [service.go:295] try to connect to server..."),
            Some(FrpcSignal::Connecting)
        );
        assert_eq!(
            classify_frpc_line("[I] [service.go:287] login to server success, get run id [abcd]"),
            Some(FrpcSignal::LoginSuccess)
        );
        let signal = classify_frpc_line(
            "[I] [proxy/proxy_manager.go:173] [minecraft-lan] start proxy success",
        )
        .expect("proxy start should be detected");
        assert!(is_minecraft_proxy_started(&signal));
    }

    #[test]
    fn parses_error_lines() {
        assert_eq!(
            classify_frpc_line("[W] token in login doesn't match token from configuration"),
            Some(FrpcSignal::LoginFailed {
                detail: "[W] token in login doesn't match token from configuration".into(),
            })
        );
        assert_eq!(
            classify_frpc_line(
                "[E] [proxy/proxy_manager.go:160] [minecraft-lan] start error: remote port already used"
            ),
            Some(FrpcSignal::StartError {
                detail: "start error: remote port already used".into(),
            })
        );
    }
}
