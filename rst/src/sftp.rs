use anyhow::{bail, Context, Result};
use russh::client::{self, Handle};
use russh::keys::PublicKey;
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

pub struct SftpClient {
    sftp: SftpSession,
    _session: Handle<Handler>,
}

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
        let sftp = SftpSession::new(channel.into_stream()).await?;

        Ok(Self { sftp, _session: session })
    }

    pub async fn list_remote_jars(&self, remote_folder: &str) -> Result<Vec<String>> {
        let dir = self.sftp.read_dir(remote_folder).await?;
        let jars = dir
            .into_iter()
            .filter(|e| e.file_name().ends_with(".jar"))
            .map(|e| e.file_name())
            .collect();
        Ok(jars)
    }

    pub async fn read_remote_file_bytes(&self, remote_path: &str) -> Result<Vec<u8>> {
        let mut file = self.sftp.open(remote_path).await?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    pub async fn upload_bytes(&self, data: &[u8], remote_path: &str) -> Result<()> {
        let mut remote = self.sftp.create(remote_path).await?;
        const CHUNK: usize = 32 * 1024;
        for chunk in data.chunks(CHUNK) {
            remote.write_all(chunk).await?;
        }
        remote.shutdown().await?;
        Ok(())
    }

    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let data = tokio::fs::read(local_path).await?;
        self.upload_bytes(&data, remote_path).await
    }

    pub async fn delete_remote_file(&self, remote_path: &str) -> Result<()> {
        self.sftp.remove_file(remote_path).await?;
        Ok(())
    }
}
