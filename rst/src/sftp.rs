use anyhow::{bail, Context, Result};
use russh::client::{self, Handle};
use russh::keys::PublicKey;
use futures_util::{stream, StreamExt, TryStreamExt};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use std::path::Path;
use std::sync::Arc;
use crate::models::ServerConfig;

struct Handler;

impl client::Handler for Handler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept any host key — acceptable for a personal tool.
        Ok(true)
    }
}

/// Single SFTP channel driven through the raw protocol so reads/writes can keep
/// many requests in flight (the high-level `SftpSession::File` does one at a time).
pub struct SftpClient {
    raw: RawSftpSession,
    read_len: u32,
    write_len: u32,
    _session: Handle<Handler>,
}

/// Concurrent READ requests per file. 64 × 256KiB = 16MiB in flight, enough to
/// saturate a link with 100ms+ RTT.
const READ_INFLIGHT: usize = 64;

impl SftpClient {
    pub async fn connect(config: &ServerConfig) -> Result<Self> {
        let ssh_config = Arc::new(client::Config::default());
        let addr = format!("{}:{}", config.host, config.port);

        let mut session = client::connect(ssh_config, addr, Handler)
            .await
            .with_context(|| format!("Cannot connect to {}", config.host))?;

        let auth = session
            .authenticate_password(&config.username, &config.password)
            .await?;

        if !auth.success() {
            bail!("SFTP authentication failed for user {}", config.username);
        }

        let channel = session.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let mut raw = RawSftpSession::new(channel.into_stream());
        let version = raw.init().await?;
        // Servers without limits@openssh.com: stay within the 32KiB packet floor.
        let mut read_len: u32 = 32 * 1024 - 9;
        let mut write_len: u32 = 32 * 1024 - 21;
        if version.extensions.get("limits@openssh.com").is_some_and(|v| v == "1") {
            let limits = raw.limits().await?;
            if limits.max_read_len > 0 {
                read_len = limits.max_read_len.min(262144 - 9) as u32;
            }
            if limits.max_write_len > 0 {
                write_len = limits.max_write_len.min(262144 - 21) as u32;
            }
            raw.set_limits(limits.into());
        }

        Ok(Self { raw, read_len, write_len, _session: session })
    }

    pub async fn list_remote_jars(&self, remote_folder: &str) -> Result<Vec<String>> {
        let handle = self.raw.opendir(remote_folder).await?.handle;
        let mut jars = Vec::new();
        loop {
            match self.raw.readdir(&handle).await {
                Ok(name) => jars.extend(
                    name.files.into_iter().map(|f| f.filename).filter(|n| n.ends_with(".jar")),
                ),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = self.raw.close(handle).await;
                    return Err(e.into());
                }
            }
        }
        let _ = self.raw.close(handle).await;
        Ok(jars)
    }

    /// Read a whole remote file with many READ requests in flight instead of one
    /// round-trip per packet.
    pub async fn read_remote_file_bytes(&self, remote_path: &str) -> Result<Vec<u8>> {
        let handle = self
            .raw
            .open(remote_path, OpenFlags::READ, FileAttributes::default())
            .await?
            .handle;
        let res = self.read_all(&handle).await;
        let _ = self.raw.close(handle).await;
        res.with_context(|| format!("Failed to read {remote_path}"))
    }

    async fn read_all(&self, handle: &str) -> Result<Vec<u8>> {
        let size = self.raw.fstat(handle).await?.attrs.size.unwrap_or(0);
        let read_len = self.read_len as u64;
        let mut buf = vec![0u8; size as usize];

        let chunks = stream::iter((0..size).step_by(read_len as usize)).map(|offset| async move {
            let want = read_len.min(size - offset) as usize;
            let mut data = Vec::with_capacity(want);
            // Servers may return short reads: loop until the chunk is complete.
            while data.len() < want {
                let d = self.raw.read(handle, offset + data.len() as u64, (want - data.len()) as u32).await?.data;
                if d.is_empty() {
                    break;
                }
                data.extend_from_slice(&d);
            }
            anyhow::Ok((offset as usize, data))
        });
        let mut chunks = chunks.buffer_unordered(READ_INFLIGHT);
        let mut got = 0usize;
        while let Some(r) = chunks.next().await {
            let (off, data) = r?;
            buf[off..off + data.len()].copy_from_slice(&data);
            got += data.len();
        }
        if got != size as usize {
            bail!("Short read: got {got} of {size} bytes");
        }
        Ok(buf)
    }

    pub async fn upload_bytes(&self, data: &[u8], remote_path: &str) -> Result<()> {
        let handle = self
            .raw
            .open(
                remote_path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                FileAttributes::default(),
            )
            .await?
            .handle;
        let wl = self.write_len as usize;
        let writes: Vec<_> = data
            .chunks(wl)
            .enumerate()
            .map(|(i, chunk)| self.raw.write(handle.clone(), (i * wl) as u64, chunk.to_vec()))
            .collect();
        let res = stream::iter(writes)
            .buffer_unordered(16)
            .try_fold((), |_, _| async { Ok(()) })
            .await;
        self.raw.close(handle).await?;
        res?;
        Ok(())
    }

    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let data = tokio::fs::read(local_path).await?;
        self.upload_bytes(&data, remote_path).await
    }

    pub async fn delete_remote_file(&self, remote_path: &str) -> Result<()> {
        self.raw.remove(remote_path).await?;
        Ok(())
    }
}
