use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::embedded_assets;

use super::{AltairZ80CpuMode, SimhLaunchConfig, SimhTarget};

pub const OPEN_SIMH_UPSTREAM_COMMIT: &str = "a1f57fa3738ed31148d31126ba1a7278ff845c6d";
pub const RUSTAIR_SIMH_BUNDLE_REVISION: &str = "a1f57fa3-rustair1";

const ALTAIR_ASSET: &str = "SIMH-backend/altair.exe";
const ALTAIRZ80_ASSET: &str = "SIMH-backend/altairz80.exe";
const FRONTPANEL_ASSET: &str = "SIMH-backend/simh_frontpanel.dll";

static CONFIG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimhRuntimePaths {
    pub root: PathBuf,
    pub altair_exe: PathBuf,
    pub altairz80_exe: PathBuf,
    pub frontpanel_dll: PathBuf,
}

#[derive(Debug)]
pub enum SimhRuntimeError {
    MissingEmbeddedAsset(&'static str),
    InvalidOverride {
        variable: &'static str,
        path: PathBuf,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for SimhRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEmbeddedAsset(path) => {
                write!(f, "embedded SIMH runtime asset is missing: {path}")
            }
            Self::InvalidOverride { variable, path } => write!(
                f,
                "{variable} points at a missing SIMH runtime file: {}",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "{operation} {} failed: {source}", path.display()),
        }
    }
}

impl std::error::Error for SimhRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn prepare_embedded_runtime() -> Result<SimhRuntimePaths, SimhRuntimeError> {
    let root = runtime_root();
    fs::create_dir_all(&root).map_err(|source| SimhRuntimeError::Io {
        operation: "create SIMH runtime directory",
        path: root.clone(),
        source,
    })?;

    let embedded_altair = root.join("altair.exe");
    let embedded_altairz80 = root.join("altairz80.exe");
    let embedded_frontpanel = root.join("simh_frontpanel.dll");

    materialize(ALTAIR_ASSET, &embedded_altair)?;
    materialize(ALTAIRZ80_ASSET, &embedded_altairz80)?;
    materialize(FRONTPANEL_ASSET, &embedded_frontpanel)?;

    let altair_exe = file_override("RUSTAIR_SIMH_ALTAIR_EXE", embedded_altair)?;
    let altairz80_exe = file_override("RUSTAIR_SIMH_ALTAIRZ80_EXE", embedded_altairz80)?;
    let frontpanel_dll = frontpanel_override(embedded_frontpanel)?;

    Ok(SimhRuntimePaths {
        root,
        altair_exe,
        altairz80_exe,
        frontpanel_dll,
    })
}

pub fn embedded_altair_launch_config() -> Result<SimhLaunchConfig, SimhRuntimeError> {
    let runtime = prepare_embedded_runtime()?;
    let config = write_launch_config(
        &runtime.root,
        "altair",
        "set cpu 8080\nset cpu 64k\n",
    )?;
    Ok(SimhLaunchConfig::new(
        SimhTarget::Altair,
        runtime.altair_exe,
        config,
    ))
}

pub fn embedded_altairz80_launch_config(
    mode: AltairZ80CpuMode,
) -> Result<SimhLaunchConfig, SimhRuntimeError> {
    let runtime = prepare_embedded_runtime()?;
    let body = format!("set cpu {}\nset cpu 64kb\n", mode.simh_modifier());
    let config = write_launch_config(
        &runtime.root,
        match mode {
            AltairZ80CpuMode::Intel8080 => "altairz80-8080",
            AltairZ80CpuMode::Z80 => "altairz80-z80",
        },
        &body,
    )?;
    Ok(SimhLaunchConfig::new(
        SimhTarget::AltairZ80,
        runtime.altairz80_exe,
        config,
    ))
}

pub(crate) fn frontpanel_dll_path() -> Result<PathBuf, SimhRuntimeError> {
    Ok(prepare_embedded_runtime()?.frontpanel_dll)
}

fn runtime_root() -> PathBuf {
    if let Some(path) = nonempty_env_path("RUSTAIR_SIMH_RUNTIME_DIR") {
        return path;
    }

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(local_app_data)
            .join("RusTair")
            .join("simh")
            .join(RUSTAIR_SIMH_BUNDLE_REVISION);
    }

    env::temp_dir()
        .join("RusTair")
        .join("simh")
        .join(RUSTAIR_SIMH_BUNDLE_REVISION)
}

fn materialize(asset: &'static str, destination: &Path) -> Result<(), SimhRuntimeError> {
    let bytes = embedded_assets::get(asset).ok_or(SimhRuntimeError::MissingEmbeddedAsset(asset))?;

    if matches!(fs::read(destination), Ok(existing) if existing == bytes) {
        return Ok(());
    }

    fs::write(destination, bytes).map_err(|source| SimhRuntimeError::Io {
        operation: "extract embedded SIMH runtime file",
        path: destination.to_path_buf(),
        source,
    })
}

fn file_override(
    variable: &'static str,
    embedded: PathBuf,
) -> Result<PathBuf, SimhRuntimeError> {
    let Some(path) = nonempty_env_path(variable) else {
        return Ok(embedded);
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(SimhRuntimeError::InvalidOverride { variable, path })
    }
}

fn frontpanel_override(embedded: PathBuf) -> Result<PathBuf, SimhRuntimeError> {
    if let Some(path) = nonempty_env_path("RUSTAIR_SIMH_FRONTPANEL_DLL") {
        return if path.is_file() {
            Ok(path)
        } else {
            Err(SimhRuntimeError::InvalidOverride {
                variable: "RUSTAIR_SIMH_FRONTPANEL_DLL",
                path,
            })
        };
    }

    if let Some(dir) = nonempty_env_path("RUSTAIR_SIMH_FRONTPANEL_DIR") {
        let path = dir.join("simh_frontpanel.dll");
        return if path.is_file() {
            Ok(path)
        } else {
            Err(SimhRuntimeError::InvalidOverride {
                variable: "RUSTAIR_SIMH_FRONTPANEL_DIR",
                path,
            })
        };
    }

    Ok(embedded)
}

fn nonempty_env_path(variable: &str) -> Option<PathBuf> {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn write_launch_config(
    root: &Path,
    stem: &str,
    machine_setup: &str,
) -> Result<PathBuf, SimhRuntimeError> {
    let sequence = CONFIG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let config_dir = root.join("configs");
    fs::create_dir_all(&config_dir).map_err(|source| SimhRuntimeError::Io {
        operation: "create SIMH config directory",
        path: config_dir.clone(),
        source,
    })?;
    let path = config_dir.join(format!(
        "{stem}-{}-{sequence}.ini",
        std::process::id()
    ));

    // FrontPanel appends and owns its REMOTE MASTER control channel. Do not
    // configure SIMH's ordinary console/Telnet here: that is a separate device
    // and caused RusTair to create a second, unnecessary console endpoint.
    fs::write(&path, machine_setup).map_err(|source| SimhRuntimeError::Io {
        operation: "write SIMH launch config",
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_revision_tracks_the_pinned_upstream_commit() {
        assert!(RUSTAIR_SIMH_BUNDLE_REVISION.starts_with(&OPEN_SIMH_UPSTREAM_COMMIT[..8]));
    }

    #[test]
    fn embedded_bundle_contains_all_three_runtime_files() {
        for path in [ALTAIR_ASSET, ALTAIRZ80_ASSET, FRONTPANEL_ASSET] {
            assert!(embedded_assets::get(path).is_some(), "missing {path}");
        }
    }

    #[test]
    fn launch_config_leaves_console_control_to_frontpanel() {
        let root = std::env::temp_dir().join(format!(
            "rustair-simh-runtime-test-{}-{}",
            std::process::id(),
            CONFIG_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = write_launch_config(&root, "test", "set cpu 8080\n").unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "set cpu 8080\n");
        assert!(!contents.to_ascii_lowercase().contains("set console"));
        assert!(!contents.to_ascii_lowercase().contains("set remote"));
        let _ = fs::remove_dir_all(root);
    }
}
