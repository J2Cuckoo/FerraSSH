//! Optional zero-knowledge vault sync.
//!
//! The server stores an opaque ciphertext blob. The master password never
//! leaves the device; the transport secret is a random token whose hash is
//! the only thing the sync node persists.

use serde::{Deserialize, Serialize};

use crate::crypto::{self, VaultKey};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBlob {
    pub account_id: String,
    pub revision: u64,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPush {
    pub account_id: String,
    pub token: String,
    pub revision: u64,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPullRequest {
    pub account_id: String,
    pub token: String,
}

pub fn wrap_snapshot(key: &VaultKey, snapshot: &[u8], account_id: &str, revision: u64) -> Result<SyncBlob> {
    Ok(SyncBlob {
        account_id: account_id.into(),
        revision,
        ciphertext: crypto::encrypt(key, snapshot)?,
    })
}

pub fn unwrap_snapshot(key: &VaultKey, blob: &SyncBlob) -> Result<Vec<u8>> {
    crypto::decrypt(key, &blob.ciphertext)
}

/// Last-write-wins with monotonic revision. Equal revisions keep the local copy.
pub fn resolve(local_rev: u64, remote_rev: u64) -> &'static str {
    match local_rev.cmp(&remote_rev) {
        std::cmp::Ordering::Less => "pull",
        std::cmp::Ordering::Greater => "push",
        std::cmp::Ordering::Equal => "keep",
    }
}

pub async fn push_http(base: &str, body: &SyncPush) -> Result<u64> {
    // Minimal HTTPS client via std isn't available; the desktop shell performs
    // the HTTP call. This helper exists so the protocol stays in one crate.
    let _ = (base, body);
    Err(Error::msg("HTTP sync is executed by the desktop/syncd layer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_key, random_salt, KdfParams};

    #[test]
    fn envelope_roundtrip() {
        let params = KdfParams { m_kib: 8, t: 1, p: 1 };
        let key = derive_key("pw", &random_salt(), &params).unwrap();
        let blob = wrap_snapshot(&key, b"{\"sessions\":[]}", "acct", 3).unwrap();
        assert_eq!(unwrap_snapshot(&key, &blob).unwrap(), b"{\"sessions\":[]}");
        assert_eq!(resolve(3, 4), "pull");
        assert_eq!(resolve(5, 4), "push");
    }
}
