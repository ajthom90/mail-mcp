use anyhow::{Context, Result};
use fs2::FileExt;
use mail_mcp_core::ipc::messages::McpEndpointInfo;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;

pub struct PidLock {
    file: File,
    path: std::path::PathBuf,
}

impl PidLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("opening pid file {}", path.display()))?;
        file.try_lock_exclusive()
            .with_context(|| format!("locking pid file {}", path.display()))?;
        let mut f = &file;
        f.set_len(0).ok();
        writeln!(f, "{}", std::process::id())?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn write_endpoint(path: &Path, info: &McpEndpointInfo) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(info)?;

    // Write to a sibling temp file with restrictive perms set at create time,
    // then atomically rename onto `path`. This avoids the TOCTOU window where
    // a vanilla `fs::write` + later `set_permissions` would briefly leave the
    // bearer-token file world-readable under the default umask.
    let mut tmp = std::ffi::OsString::from(
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("endpoint path has no file name"))?,
    );
    tmp.push(format!(".tmp.{}", std::process::id()));
    let tmp_path = parent.join(tmp);

    {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp_path)
            .with_context(|| format!("creating endpoint temp file {}", tmp_path.display()))?;
        f.write_all(&bytes)?;
        f.sync_all().ok();
    }
    // Atomic on POSIX. On Windows std::fs::rename will replace the destination
    // if it exists (since 1.5.0).
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} → {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Build an MCP endpoint URL from the bound HTTP server addr.
pub fn endpoint_url(addr: SocketAddr) -> String {
    format!("http://{addr}/mcp")
}

/// Generate a fresh random bearer token (32 bytes, base64url, 43 chars).
pub fn fresh_bearer_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_mcp_core::paths::Paths;

    #[test]
    fn pid_lock_prevents_second_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::with_root(tmp.path().to_path_buf());
        p.ensure_dirs().unwrap();
        let _first = PidLock::acquire(&p.pid_file()).unwrap();
        let second = PidLock::acquire(&p.pid_file());
        assert!(second.is_err());
    }

    #[test]
    fn endpoint_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ep.json");
        let info = McpEndpointInfo {
            url: "http://127.0.0.1:65000/mcp".into(),
            bearer_token: "abc".into(),
            stdio_shim_path: Some("/usr/local/bin/mail-mcp-stdio".into()),
        };
        write_endpoint(&path, &info).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let loaded: McpEndpointInfo =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded.url, info.url);
        assert_eq!(loaded.bearer_token, info.bearer_token);
    }

    /// Regression for the v0.1a TOCTOU bug: an existing endpoint file with the
    /// wrong perms must NOT be writable in any world-readable intermediate
    /// state. The atomic-rename path means the destination is either the old
    /// 0o600 file or the new 0o600 file — never a 0o644 in-flight write.
    #[cfg(unix)]
    #[test]
    fn endpoint_overwrite_keeps_perms_tight() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ep.json");

        // Pre-seed the destination with 0o644 to simulate a stale file.
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let info = McpEndpointInfo {
            url: "http://127.0.0.1:65000/mcp".into(),
            bearer_token: "secret".into(),
            stdio_shim_path: None,
        };
        write_endpoint(&path, &info).unwrap();
        // The file exists, has the new contents, and is 0o600.
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "endpoint.json should be 0o600 after overwrite");
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("secret"));
        // No leftover *.tmp.* sibling.
        let strays: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(strays.is_empty(), "temp file leaked: {strays:?}");
    }
}
