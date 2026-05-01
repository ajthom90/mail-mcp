use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Filesystem layout for a single user installation of mail-mcp.
///
/// On macOS:
///   data:    ~/Library/Application Support/mail-mcp/
///   logs:    ~/Library/Logs/mail-mcp/
///   cache:   ~/Library/Caches/mail-mcp/
///   runtime: ${TMPDIR}mail-mcp-<uid>/
///
/// On Linux:
///   data:    ${XDG_DATA_HOME:-~/.local/share}/mail-mcp/
///   logs:    ${XDG_STATE_HOME:-~/.local/state}/mail-mcp/logs/
///   cache:   ${XDG_CACHE_HOME:-~/.cache}/mail-mcp/
///   runtime: ${XDG_RUNTIME_DIR:-/run/user/<uid>}/mail-mcp/
///
/// On Windows:
///   data:    %LOCALAPPDATA%\mail-mcp\
///   logs:    %LOCALAPPDATA%\mail-mcp\logs\
///   cache:   %LOCALAPPDATA%\mail-mcp\cache\
///   runtime: (named pipes use \\.\pipe\mail-mcp-<sid> instead)
#[derive(Debug, Clone)]
pub struct Paths {
    data: PathBuf,
    logs: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
}

impl Paths {
    /// Compute paths for the current user using platform conventions.
    pub fn default_for_user() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| Error::Config("no HOME dir".into()))?;
            let data = home.join("Library/Application Support/mail-mcp");
            let logs = home.join("Library/Logs/mail-mcp");
            let cache = home.join("Library/Caches/mail-mcp");
            let tmpdir = std::env::var_os("TMPDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            let uid = unsafe { libc_geteuid() };
            let runtime = tmpdir.join(format!("mail-mcp-{uid}"));
            Ok(Self { data, logs, cache, runtime })
        }
        #[cfg(target_os = "linux")]
        {
            let data = dirs::data_dir()
                .ok_or_else(|| Error::Config("no XDG data dir".into()))?
                .join("mail-mcp");
            let logs = dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .ok_or_else(|| Error::Config("no XDG state dir".into()))?
                .join("mail-mcp/logs");
            let cache = dirs::cache_dir()
                .ok_or_else(|| Error::Config("no XDG cache dir".into()))?
                .join("mail-mcp");
            let runtime = dirs::runtime_dir()
                .map(|d| d.join("mail-mcp"))
                .unwrap_or_else(|| {
                    let uid = unsafe { libc_geteuid() };
                    PathBuf::from(format!("/tmp/mail-mcp-{uid}"))
                });
            Ok(Self { data, logs, cache, runtime })
        }
        #[cfg(target_os = "windows")]
        {
            let local = dirs::data_local_dir()
                .ok_or_else(|| Error::Config("no LOCALAPPDATA".into()))?
                .join("mail-mcp");
            Ok(Self {
                data: local.clone(),
                logs: local.join("logs"),
                cache: local.join("cache"),
                runtime: local.join("run"),
            })
        }
    }

    /// Override all paths for tests / portable installs.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            data: root.join("data"),
            logs: root.join("logs"),
            cache: root.join("cache"),
            runtime: root.join("run"),
        }
    }

    pub fn data_dir(&self) -> &Path { &self.data }
    pub fn logs_dir(&self) -> &Path { &self.logs }
    pub fn cache_dir(&self) -> &Path { &self.cache }
    pub fn runtime_dir(&self) -> &Path { &self.runtime }

    pub fn state_db(&self) -> PathBuf { self.data.join("state.db") }
    pub fn endpoint_json(&self) -> PathBuf { self.data.join("endpoint.json") }
    pub fn ipc_socket(&self) -> PathBuf { self.runtime.join("ipc.sock") }
    pub fn pid_file(&self) -> PathBuf { self.runtime.join("daemon.pid") }

    /// Ensure all directories exist with permissions appropriate to their content.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data)?;
        std::fs::create_dir_all(&self.logs)?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.runtime)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perm = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(&self.runtime, perm.clone())?;
            std::fs::set_permissions(&self.data, perm)?;
        }
        Ok(())
    }
}

#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_use_temp_root_when_overridden() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::with_root(tmp.path().to_path_buf());
        assert_eq!(p.data_dir(), tmp.path().join("data"));
        assert_eq!(p.state_db(), tmp.path().join("data").join("state.db"));
        assert_eq!(p.endpoint_json(), tmp.path().join("data").join("endpoint.json"));
        assert_eq!(p.logs_dir(), tmp.path().join("logs"));
        assert_eq!(p.cache_dir(), tmp.path().join("cache"));
        assert_eq!(p.runtime_dir(), tmp.path().join("run"));
        assert_eq!(p.ipc_socket(), tmp.path().join("run").join("ipc.sock"));
        assert_eq!(p.pid_file(), tmp.path().join("run").join("daemon.pid"));
    }

    #[test]
    fn ensure_dirs_creates_all() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::with_root(tmp.path().to_path_buf());
        p.ensure_dirs().unwrap();
        assert!(p.data_dir().is_dir());
        assert!(p.logs_dir().is_dir());
        assert!(p.cache_dir().is_dir());
        assert!(p.runtime_dir().is_dir());
    }

    #[test]
    fn default_paths_use_platform_dirs() {
        let p = Paths::default_for_user().unwrap();
        // Sanity: every leaf is descended from a non-empty root.
        assert!(p.data_dir().is_absolute());
        assert!(p.logs_dir().is_absolute());
        assert!(p.cache_dir().is_absolute());
        assert!(p.runtime_dir().is_absolute());
    }
}
