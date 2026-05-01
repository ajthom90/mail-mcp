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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(info)?;
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
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
}
