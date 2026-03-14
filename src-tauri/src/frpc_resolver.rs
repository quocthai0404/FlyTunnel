use crate::{
    event_sink::SharedEventSink,
    models::{
        FrpBinaryStatus, FrpcDownloadPayload, FrpcProbe, FrpcResolution, FRP_VERSION_NUMBER,
        FRP_VERSION_TAG,
    },
    settings,
};
use reqwest::blocking::Client;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrpBinaryKind {
    Client,
    Server,
}

impl FrpBinaryKind {
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Client => {
                if cfg!(windows) {
                    "frpc.exe"
                } else {
                    "frpc"
                }
            }
            Self::Server => {
                if cfg!(windows) {
                    "frps.exe"
                } else {
                    "frps"
                }
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Client => "frpc",
            Self::Server => "frps",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSpec {
    pub archive_name: &'static str,
    pub binary_name: &'static str,
    pub archive_kind: ArchiveKind,
}

pub fn probe_frpc(override_path: Option<&str>) -> Result<FrpcProbe, String> {
    probe_binary(FrpBinaryKind::Client, override_path)
}

pub fn ensure_frpc(
    sink: SharedEventSink,
    override_path: Option<&str>,
) -> Result<FrpcResolution, String> {
    ensure_binary(FrpBinaryKind::Client, sink, override_path)
}

pub fn ensure_host_binary(
    kind: FrpBinaryKind,
    sink: SharedEventSink,
) -> Result<FrpBinaryStatus, String> {
    ensure_binary(kind, sink, None)
}

pub fn pick_frpc_binary() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Locate frpc")
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

pub fn asset_for(kind: FrpBinaryKind, os: &str, arch: &str) -> Result<AssetSpec, String> {
    let binary_name = match (kind, os) {
        (FrpBinaryKind::Client, "windows") => "frpc.exe",
        (FrpBinaryKind::Client, _) => "frpc",
        (FrpBinaryKind::Server, "windows") => "frps.exe",
        (FrpBinaryKind::Server, _) => "frps",
    };

    let archive = match (os, arch) {
        ("windows", "x86_64") => ("frp_0.67.0_windows_amd64.zip", ArchiveKind::Zip),
        ("windows", "aarch64") => ("frp_0.67.0_windows_arm64.zip", ArchiveKind::Zip),
        ("macos", "x86_64") => ("frp_0.67.0_darwin_amd64.tar.gz", ArchiveKind::TarGz),
        ("macos", "aarch64") => ("frp_0.67.0_darwin_arm64.tar.gz", ArchiveKind::TarGz),
        ("linux", "x86_64") => ("frp_0.67.0_linux_amd64.tar.gz", ArchiveKind::TarGz),
        ("linux", "aarch64") => ("frp_0.67.0_linux_arm64.tar.gz", ArchiveKind::TarGz),
        _ => {
            return Err(format!(
                "Unsupported platform for official frp binary auto-download: {os}/{arch}"
            ))
        }
    };

    Ok(AssetSpec {
        archive_name: archive.0,
        binary_name,
        archive_kind: archive.1,
    })
}

pub fn cached_binary_path(kind: FrpBinaryKind) -> Result<PathBuf, String> {
    let spec = asset_for(kind, std::env::consts::OS, std::env::consts::ARCH)?;
    Ok(settings::bin_root()?
        .join(FRP_VERSION_TAG)
        .join(spec.binary_name))
}

fn probe_binary(
    kind: FrpBinaryKind,
    override_path: Option<&str>,
) -> Result<FrpBinaryStatus, String> {
    if let Some(path) = normalize_override_path(override_path) {
        if path.is_file() {
            return Ok(binary_ready_status(
                kind,
                path,
                "manual",
                format!("Using manually selected {} binary.", kind.label()),
            ));
        }

        return Ok(FrpBinaryStatus {
            ready: false,
            path: None,
            source: None,
            version: FRP_VERSION_TAG.into(),
            display_message: format!(
                "Saved {} path was not found. FlyTunnel will download the official binary when you start.",
                kind.label()
            ),
        });
    }

    let cached_path = cached_binary_path(kind)?;
    if cached_path.is_file() {
        return Ok(binary_ready_status(
            kind,
            cached_path,
            "cached",
            format!(
                "Using cached official {} {} binary.",
                kind.label(),
                FRP_VERSION_NUMBER
            ),
        ));
    }

    Ok(FrpBinaryStatus {
        ready: false,
        path: None,
        source: None,
        version: FRP_VERSION_TAG.into(),
        display_message: format!(
            "Official {} {} will be downloaded when you start the tunnel.",
            kind.label(),
            FRP_VERSION_NUMBER
        ),
    })
}

fn ensure_binary(
    kind: FrpBinaryKind,
    sink: SharedEventSink,
    override_path: Option<&str>,
) -> Result<FrpBinaryStatus, String> {
    emit_state(
        &sink,
        "checking",
        &format!("Checking {} availability...", kind.label()),
        None,
    );

    if let Some(path) = normalize_override_path(override_path) {
        if path.is_file() {
            emit_state(
                &sink,
                "ready",
                &format!("Using manually selected {} binary.", kind.label()),
                Some(path.to_string_lossy().into_owned()),
            );
            return Ok(binary_ready_status(
                kind,
                path,
                "manual",
                format!("Using manually selected {} binary.", kind.label()),
            ));
        }

        emit_state(
            &sink,
            "missing",
            &format!(
                "Saved {} override was not found. Falling back to official download.",
                kind.label()
            ),
            None,
        );
    }

    let spec = asset_for(kind, std::env::consts::OS, std::env::consts::ARCH)?;
    let install_dir = settings::bin_root()?.join(FRP_VERSION_TAG);
    fs::create_dir_all(&install_dir)
        .map_err(|error| format!("Failed to prepare {} directory: {error}", kind.label()))?;
    let binary_path = install_dir.join(spec.binary_name);

    if binary_path.is_file() {
        emit_state(
            &sink,
            "cached",
            &format!("Using cached {} binary.", kind.label()),
            Some(binary_path.to_string_lossy().into_owned()),
        );
        return Ok(binary_ready_status(
            kind,
            binary_path,
            "cached",
            format!(
                "Using cached official {} {} binary.",
                kind.label(),
                FRP_VERSION_NUMBER
            ),
        ));
    }

    let archive_path = install_dir.join(spec.archive_name);
    let download_url = format!(
        "https://github.com/fatedier/frp/releases/download/{}/{}",
        FRP_VERSION_TAG, spec.archive_name
    );

    emit_state(
        &sink,
        "downloading",
        &format!(
            "Downloading official {} {}...",
            kind.label(),
            spec.archive_name
        ),
        None,
    );
    download_archive(&download_url, &archive_path, kind)?;

    emit_state(
        &sink,
        "extracting",
        &format!("Extracting {} binary...", kind.label()),
        None,
    );
    extract_binary(&archive_path, &binary_path, &spec, kind)?;
    let _ = fs::remove_file(&archive_path);

    emit_state(
        &sink,
        "ready",
        &format!("{} is ready to use.", kind.label()),
        Some(binary_path.to_string_lossy().into_owned()),
    );

    Ok(binary_ready_status(
        kind,
        binary_path,
        "downloaded",
        format!(
            "Downloaded official {} {} for this machine.",
            kind.label(),
            FRP_VERSION_NUMBER
        ),
    ))
}

fn binary_ready_status(
    _kind: FrpBinaryKind,
    path: PathBuf,
    source: &str,
    display_message: String,
) -> FrpBinaryStatus {
    FrpBinaryStatus {
        ready: true,
        path: Some(path.to_string_lossy().into_owned()),
        source: Some(source.into()),
        version: FRP_VERSION_TAG.into(),
        display_message,
    }
}

fn normalize_override_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn download_archive(url: &str, destination: &Path, kind: FrpBinaryKind) -> Result<(), String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| format!("Failed to create HTTP client for {}: {error}", kind.label()))?;
    let mut response = client
        .get(url)
        .header("User-Agent", "FlyTunnel/0.1")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Failed to download {}: {error}", kind.label()))?;
    let mut file = File::create(destination)
        .map_err(|error| format!("Failed to create {} archive: {error}", kind.label()))?;

    response
        .copy_to(&mut file)
        .map_err(|error| format!("Failed to write {} archive: {error}", kind.label()))?;

    Ok(())
}

fn extract_binary(
    archive_path: &Path,
    binary_path: &Path,
    spec: &AssetSpec,
    kind: FrpBinaryKind,
) -> Result<(), String> {
    if binary_path.exists() {
        fs::remove_file(binary_path)
            .map_err(|error| format!("Failed to replace {} binary: {error}", kind.label()))?;
    }

    match spec.archive_kind {
        ArchiveKind::Zip => extract_from_zip(archive_path, binary_path, spec.binary_name, kind)?,
        ArchiveKind::TarGz => {
            extract_from_tar_gz(archive_path, binary_path, spec.binary_name, kind)?
        }
    }

    #[cfg(unix)]
    {
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(binary_path, permissions)
            .map_err(|error| format!("Failed to set {} executable bit: {error}", kind.label()))?;
    }

    Ok(())
}

fn extract_from_zip(
    archive_path: &Path,
    binary_path: &Path,
    binary_name: &str,
    kind: FrpBinaryKind,
) -> Result<(), String> {
    let archive = File::open(archive_path)
        .map_err(|error| format!("Failed to open {} archive: {error}", kind.label()))?;
    let mut zip = zip::ZipArchive::new(archive)
        .map_err(|error| format!("Failed to inspect {} archive: {error}", kind.label()))?;

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("Failed to read {} archive entry: {error}", kind.label()))?;
        let matches = Path::new(entry.name())
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case(binary_name))
            .unwrap_or(false);

        if matches {
            let mut output = File::create(binary_path)
                .map_err(|error| format!("Failed to create {} binary: {error}", kind.label()))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("Failed to extract {} binary: {error}", kind.label()))?;
            return Ok(());
        }
    }

    Err(format!(
        "{} binary was not found inside the downloaded archive.",
        kind.label()
    ))
}

fn extract_from_tar_gz(
    archive_path: &Path,
    binary_path: &Path,
    binary_name: &str,
    kind: FrpBinaryKind,
) -> Result<(), String> {
    let archive = File::open(archive_path)
        .map_err(|error| format!("Failed to open {} archive: {error}", kind.label()))?;
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut tar_archive = tar::Archive::new(decoder);
    let entries = tar_archive
        .entries()
        .map_err(|error| format!("Failed to inspect {} archive: {error}", kind.label()))?;

    for entry in entries {
        let mut entry = entry
            .map_err(|error| format!("Failed to read {} archive entry: {error}", kind.label()))?;
        let matches = entry
            .path()
            .map_err(|error| {
                format!(
                    "Failed to resolve {} archive entry path: {error}",
                    kind.label()
                )
            })?
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value == binary_name)
            .unwrap_or(false);

        if matches {
            let mut output = File::create(binary_path)
                .map_err(|error| format!("Failed to create {} binary: {error}", kind.label()))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("Failed to extract {} binary: {error}", kind.label()))?;
            return Ok(());
        }
    }

    Err(format!(
        "{} binary was not found inside the downloaded archive.",
        kind.label()
    ))
}

fn emit_state(sink: &SharedEventSink, stage: &str, message: &str, path: Option<String>) {
    sink.emit_download(FrpcDownloadPayload {
        stage: stage.into(),
        message: message.into(),
        path,
    });
}

#[cfg(test)]
mod tests {
    use super::{asset_for, cached_binary_path, probe_frpc, ArchiveKind, FrpBinaryKind};
    use std::{
        fs,
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_test_roots(root: &TempDir) {
        let config_root = root.path().join("config");
        let data_root = root.path().join("data");
        std::env::set_var("FLYTUNNEL_CONFIG_DIR", config_root);
        std::env::set_var("FLYTUNNEL_DATA_DIR", data_root);
    }

    fn clear_test_roots() {
        std::env::remove_var("FLYTUNNEL_CONFIG_DIR");
        std::env::remove_var("FLYTUNNEL_DATA_DIR");
    }

    #[test]
    fn maps_client_windows_asset() {
        let asset =
            asset_for(FrpBinaryKind::Client, "windows", "x86_64").expect("asset should exist");
        assert_eq!(asset.archive_name, "frp_0.67.0_windows_amd64.zip");
        assert_eq!(asset.binary_name, "frpc.exe");
        assert_eq!(asset.archive_kind, ArchiveKind::Zip);
    }

    #[test]
    fn maps_server_macos_asset() {
        let asset =
            asset_for(FrpBinaryKind::Server, "macos", "aarch64").expect("asset should exist");
        assert_eq!(asset.archive_name, "frp_0.67.0_darwin_arm64.tar.gz");
        assert_eq!(asset.binary_name, "frps");
        assert_eq!(asset.archive_kind, ArchiveKind::TarGz);
    }

    #[test]
    fn probe_does_not_download_when_binary_is_missing() {
        let _guard = env_lock().lock().expect("env lock");
        let root = TempDir::new().expect("temp dir");
        set_test_roots(&root);

        let probe = probe_frpc(None).expect("probe should succeed");
        let expected = cached_binary_path(FrpBinaryKind::Client).expect("cached path");
        let install_dir = expected.parent().map(PathBuf::from).expect("install dir");
        let bin_root = install_dir.parent().map(PathBuf::from).expect("bin root");

        assert!(!probe.ready);
        assert!(probe.path.is_none());
        assert!(bin_root.exists());
        assert!(!expected.exists());
        assert!(
            !install_dir.exists() || fs::read_dir(&install_dir).expect("install dir").count() == 0
        );

        clear_test_roots();
    }
}
