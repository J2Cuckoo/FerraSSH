use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error("vault is locked")]
    VaultLocked,

    #[error("vault is not initialized")]
    VaultMissing,

    #[error("incorrect master password")]
    BadPassword,

    #[error("host key mismatch for {host}:{port} (expected {expected}, got {actual})")]
    HostKeyMismatch {
        host: String,
        port: u16,
        expected: String,
        actual: String,
    },

    #[error("unknown host key for {host}:{port}: {fingerprint}")]
    UnknownHostKey { host: String, port: u16, fingerprint: String },

    #[error("authentication failed")]
    AuthFailed,

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("sftp is not open on this session")]
    SftpClosed,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Ssh(#[from] russh::Error),

    #[error(transparent)]
    Sftp(#[from] russh_sftp::client::error::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("国密加解密失败")]
    Crypto,

    #[error("kdf error: {0}")]
    Kdf(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    pub fn to_ui(&self) -> String {
        match self {
            Error::UnknownHostKey { host, port, fingerprint } => serde_json::json!({
                "code": "unknown_host_key",
                "host": host,
                "port": port,
                "fingerprint": fingerprint,
                "message": self.to_string(),
            })
            .to_string(),
            Error::HostKeyMismatch { host, port, expected, actual } => serde_json::json!({
                "code": "host_key_mismatch",
                "host": host,
                "port": port,
                "expected": expected,
                "fingerprint": actual,
                "message": self.to_string(),
            })
            .to_string(),
            other => Self::sftp_ui(other.to_string()),
        }
    }

    fn sftp_ui(raw: String) -> String {
        let lower = raw.to_ascii_lowercase();
        if lower.contains("permission denied") || lower.contains("permissiondenied") {
            return format!("没有权限：{raw}");
        }
        if lower.contains("no such file") {
            return format!("路径不存在：{raw}");
        }
        if lower.contains("quota") || lower.contains("no space") {
            return format!("空间不足：{raw}");
        }
        raw
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denied_is_readable() {
        let s = Error::msg("Permission denied: Permission denied").to_ui();
        assert!(s.contains("没有权限"), "{s}");
    }
}
