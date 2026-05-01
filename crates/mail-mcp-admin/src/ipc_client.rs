use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub struct IpcClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    next_id: u64,
}

impl IpcClient {
    pub async fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .await
            .with_context(|| format!("connecting to {}", socket.display()))?;
        let (rx, tx) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(rx),
            writer: tx,
            next_id: 1,
        })
    }

    pub async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let req = serde_json::json!({"jsonrpc":"2.0", "id": id, "method": method, "params": params});
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        // Read responses until we see one with our id (notifications may interleave).
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self.reader.read_line(&mut buf).await?;
            if n == 0 {
                bail!("daemon closed connection");
            }
            #[derive(Deserialize)]
            struct Resp {
                #[serde(default)]
                id: serde_json::Value,
                result: Option<serde_json::Value>,
                error: Option<RespErr>,
            }
            #[derive(Deserialize)]
            struct RespErr {
                code: i64,
                message: String,
            }
            let resp: Resp = serde_json::from_str(buf.trim())?;
            if resp.id.as_u64() != Some(id) {
                continue;
            }
            if let Some(err) = resp.error {
                return Err(anyhow!("daemon error ({}): {}", err.code, err.message));
            }
            return Ok(resp.result.ok_or_else(|| anyhow!("response missing result"))?);
        }
    }

    pub async fn read_notification(&mut self) -> Result<serde_json::Value> {
        let mut buf = String::new();
        let n = self.reader.read_line(&mut buf).await?;
        if n == 0 {
            bail!("daemon closed connection");
        }
        Ok(serde_json::from_str(buf.trim())?)
    }
}
