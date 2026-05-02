//! Cross-platform transport for the IPC server / client.
//!
//! On Unix the IPC channel is a Unix domain socket at `<runtime>/ipc.sock`.
//! On Windows it's a named pipe at `\\.\pipe\mail-mcp-<USERNAME>`. The two
//! APIs differ enough that we expose a small abstraction here:
//!
//!   * [`IpcStream`] — a single bidirectional connection. Implements
//!     `tokio::io::AsyncRead + AsyncWrite + Unpin + Send`. Wraps a
//!     `tokio::net::UnixStream` on Unix and a
//!     `tokio::net::windows::named_pipe::NamedPipeServer` (server-side) or
//!     `NamedPipeClient` (client-side) on Windows.
//!   * [`IpcListener::bind`] / [`IpcListener::accept`] — server-side. Mirrors
//!     `tokio::net::UnixListener` semantics on Unix; on Windows it pre-creates
//!     the next named-pipe instance and waits for a client to connect.
//!   * [`IpcStream::connect`] — client-side dial.
//!
//! The transport is **per-user**: Unix sockets get 0o600 perms, Windows pipes
//! reject remote clients.

use std::io;
use std::path::Path;

#[cfg(unix)]
mod imp {
    use super::*;
    use tokio::net::{UnixListener, UnixStream};

    pub struct IpcListener {
        inner: UnixListener,
    }

    pub struct IpcStream {
        inner: UnixStream,
    }

    impl IpcListener {
        /// Bind a fresh listener at `path`. Removes any stale socket file
        /// first and tightens parent-dir + socket perms to 0o700 / 0o600.
        pub fn bind(path: &Path) -> io::Result<Self> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            let inner = UnixListener::bind(path)?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            Ok(Self { inner })
        }

        pub async fn accept(&self) -> io::Result<IpcStream> {
            let (sock, _addr) = self.inner.accept().await?;
            Ok(IpcStream { inner: sock })
        }
    }

    impl IpcStream {
        pub async fn connect(path: &Path) -> io::Result<Self> {
            let inner = UnixStream::connect(path).await?;
            Ok(Self { inner })
        }
    }

    impl tokio::io::AsyncRead for IpcStream {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for IpcStream {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<io::Result<usize>> {
            std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    /// Server-side listener. Owns the path and (re-)creates a fresh
    /// `NamedPipeServer` instance for each accept.
    pub struct IpcListener {
        path: std::ffi::OsString,
        // Holds the next, pre-created server instance. Windows requires us to
        // create the instance BEFORE a client tries to connect — otherwise
        // the connect attempt fails with FILE_NOT_FOUND. We refill it after
        // each accept.
        next: Mutex<Option<NamedPipeServer>>,
    }

    impl IpcListener {
        pub fn bind(path: &Path) -> io::Result<Self> {
            let path = path.as_os_str().to_owned();
            // Pre-create the first instance so clients can connect immediately.
            let first = ServerOptions::new()
                .reject_remote_clients(true)
                .first_pipe_instance(true)
                .create(&path)?;
            Ok(Self {
                path,
                next: Mutex::new(Some(first)),
            })
        }

        pub async fn accept(&self) -> io::Result<IpcStream> {
            // Take the pre-created instance and arm it.
            let server = {
                let mut g = self.next.lock().unwrap();
                g.take()
                    .ok_or_else(|| io::Error::other("listener instance missing"))?
            };
            // Create the NEXT instance now so the next caller of accept()
            // doesn't race with a client connecting between accepts.
            {
                let next = ServerOptions::new()
                    .reject_remote_clients(true)
                    .create(&self.path)?;
                *self.next.lock().unwrap() = Some(next);
            }
            // Wait for a client to connect to the instance we took.
            server.connect().await?;
            Ok(IpcStream {
                inner: ServerOrClient::Server(server),
            })
        }
    }

    enum ServerOrClient {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    pub struct IpcStream {
        inner: ServerOrClient,
    }

    impl IpcStream {
        pub async fn connect(path: &Path) -> io::Result<Self> {
            // ClientOptions::open is sync. Retry briefly if all server
            // instances are busy (ERROR_PIPE_BUSY = 231).
            let path = path.as_os_str();
            for _ in 0..50 {
                match ClientOptions::new().open(path) {
                    Ok(client) => {
                        return Ok(Self {
                            inner: ServerOrClient::Client(client),
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(231) => {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::other("named pipe busy after retries"))
        }
    }

    impl AsyncRead for IpcStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match &mut self.inner {
                ServerOrClient::Server(s) => Pin::new(s).poll_read(cx, buf),
                ServerOrClient::Client(c) => Pin::new(c).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for IpcStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            match &mut self.inner {
                ServerOrClient::Server(s) => Pin::new(s).poll_write(cx, buf),
                ServerOrClient::Client(c) => Pin::new(c).poll_write(cx, buf),
            }
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match &mut self.inner {
                ServerOrClient::Server(s) => Pin::new(s).poll_flush(cx),
                ServerOrClient::Client(c) => Pin::new(c).poll_flush(cx),
            }
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match &mut self.inner {
                ServerOrClient::Server(s) => Pin::new(s).poll_shutdown(cx),
                ServerOrClient::Client(c) => Pin::new(c).poll_shutdown(cx),
            }
        }
    }
}

pub use imp::{IpcListener, IpcStream};
