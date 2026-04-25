import paramiko
from models import AppConfig
import local_mods


class SFTPClient:
    def __init__(self, config: AppConfig):
        self._config = config
        self._ssh = None
        self._sftp = None

    def __enter__(self) -> "SFTPClient":
        config = self._config
        self._ssh = paramiko.SSHClient()
        # AutoAddPolicy accepts any host key — acceptable for a personal tool.
        # For stricter security use RejectPolicy with a known_hosts file.
        self._ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        connect_kwargs = dict(
            hostname=config.sftp_host,
            port=config.sftp_port,
            username=config.sftp_username,
        )
        if config.sftp_key_path:
            connect_kwargs["key_filename"] = config.sftp_key_path
        else:
            connect_kwargs["password"] = config.sftp_password

        self._ssh.connect(**connect_kwargs)
        self._sftp = self._ssh.open_sftp()
        return self

    def __exit__(self, *args):
        if self._sftp:
            self._sftp.close()
        if self._ssh:
            self._ssh.close()

    def list_remote_jars(self, remote_folder: str) -> list:
        entries = self._sftp.listdir(remote_folder)
        return [e for e in entries if e.lower().endswith(".jar")]

    def read_remote_file_bytes(self, remote_path: str) -> bytes:
        with self._sftp.open(remote_path, "rb") as f:
            return f.read()

    def get_remote_file_hash(self, remote_path: str) -> str:
        """Calculate SHA512 hash of remote file server-side using sha512sum command."""
        try:
            stdin, stdout, stderr = self._ssh.exec_command(f"sha512sum '{remote_path}'")
            output = stdout.read().decode().strip()
            if output:
                # sha512sum output format: "hash  filename"
                return output.split()[0]
        except Exception:
            pass
        # Fallback: download and hash locally if server-side command fails
        data = self.read_remote_file_bytes(remote_path)
        return local_mods.hash_bytes(data)

    def upload_file(self, local_path: str, remote_path: str, progress_callback=None):
        self._sftp.put(local_path, remote_path, callback=progress_callback)

    def delete_remote_file(self, remote_path: str):
        self._sftp.remove(remote_path)
