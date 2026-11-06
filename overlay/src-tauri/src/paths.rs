//! Where her files live. The plugin passes the exact locations when it starts her;
//! these fallbacks match Hermes' own defaults for when she is run by hand.

use std::path::PathBuf;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// `HERMES_HOME`, else Hermes' platform default: `%LOCALAPPDATA%\hermes` on Windows,
/// `~/.hermes` elsewhere.
pub fn hermes_home() -> PathBuf {
    if let Some(home) = env_path("HERMES_HOME") {
        return home;
    }
    #[cfg(windows)]
    {
        let local = env_path("LOCALAPPDATA").or_else(|| {
            env_path("USERPROFILE").map(|profile| profile.join("AppData").join("Local"))
        });
        local.unwrap_or_else(|| PathBuf::from(".")).join("hermes")
    }
    #[cfg(not(windows))]
    {
        env_path("HOME").unwrap_or_else(|| PathBuf::from(".")).join(".hermes")
    }
}

/// Her own data (`overlay.json`, the Windows bridge file): `HERMESCHAN_STATE_DIR` from
/// the plugin, else `<hermes home>/hermes-chan`.
pub fn state_dir() -> PathBuf {
    env_path("HERMESCHAN_STATE_DIR").unwrap_or_else(|| hermes_home().join("hermes-chan"))
}

/// The plugin's AF_UNIX socket: `HERMESCHAN_SOCKET`, else `<hermes home>/hermes-chan.sock`.
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    env_path("HERMESCHAN_SOCKET").unwrap_or_else(|| hermes_home().join("hermes-chan.sock"))
}
