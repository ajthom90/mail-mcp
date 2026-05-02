//! IPC protocol over Unix domain socket / named pipe. JSON-RPC 2.0 with
//! line-delimited framing (each frame is one JSON object terminated by '\n').

pub mod messages;
pub mod server;
pub mod transport;

pub use messages::*;
pub use transport::{IpcListener, IpcStream};
