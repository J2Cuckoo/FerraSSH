//! Algorithm profiles for modern hosts and ancient network gear.
//!
//! russh disables 3DES / SHA-1 by default. FerraSSH exposes explicit
//! whitelists so a Cisco-era switch can still handshake without weakening
//! every other session.

use std::borrow::Cow;

use russh::keys::{Algorithm, EcdsaCurve, HashAlg};
use russh::{cipher, kex, mac, Preferred};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlgProfileKind {
    /// russh defaults: modern AEAD, no SHA-1, no 3DES.
    #[default]
    Modern,
    /// Adds AES-CBC and older KEX while keeping AEAD first.
    Compatible,
    /// Includes 3DES, hmac-sha1, dh-group1/14-sha1, ssh-rsa for old switches.
    Legacy,
    /// Caller-supplied ordered whitelist.
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlgSpec {
    pub profile: AlgProfileKind,
    #[serde(default)]
    pub kex: Vec<String>,
    #[serde(default)]
    pub cipher: Vec<String>,
    #[serde(default)]
    pub mac: Vec<String>,
    #[serde(default)]
    pub host_key: Vec<String>,
}

impl AlgSpec {
    pub fn modern() -> Self {
        Self { profile: AlgProfileKind::Modern, ..Self::default() }
    }

    pub fn compatible() -> Self {
        Self { profile: AlgProfileKind::Compatible, ..Self::default() }
    }

    pub fn legacy() -> Self {
        Self { profile: AlgProfileKind::Legacy, ..Self::default() }
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self.profile, AlgProfileKind::Legacy)
            || self.cipher.iter().any(|c| c.contains("3des") || c.contains("cbc"))
            || self.mac.iter().any(|m| m.contains("sha1"))
            || self.kex.iter().any(|k| k.contains("sha1") || k.contains("group1"))
    }

    pub fn to_preferred(&self) -> Result<Preferred> {
        let mut preferred = Preferred::default();
        match self.profile {
            AlgProfileKind::Modern => {}
            AlgProfileKind::Compatible => {
                preferred.kex = Cow::Borrowed(COMPAT_KEX);
                preferred.cipher = Cow::Borrowed(COMPAT_CIPHER);
                preferred.mac = Cow::Borrowed(COMPAT_MAC);
                preferred.key = Cow::Owned(compat_host_keys());
            }
            AlgProfileKind::Legacy => {
                preferred.kex = Cow::Borrowed(LEGACY_KEX);
                preferred.cipher = Cow::Borrowed(LEGACY_CIPHER);
                preferred.mac = Cow::Borrowed(LEGACY_MAC);
                preferred.key = Cow::Owned(legacy_host_keys());
            }
            AlgProfileKind::Custom => {
                if self.kex.is_empty() && self.cipher.is_empty() && self.mac.is_empty() {
                    return Err(Error::msg("custom algorithm profile is empty"));
                }
                if !self.kex.is_empty() {
                    preferred.kex = Cow::Owned(map_kex(&self.kex)?);
                }
                if !self.cipher.is_empty() {
                    preferred.cipher = Cow::Owned(map_cipher(&self.cipher)?);
                }
                if !self.mac.is_empty() {
                    preferred.mac = Cow::Owned(map_mac(&self.mac)?);
                }
                if !self.host_key.is_empty() {
                    preferred.key = Cow::Owned(map_host_key(&self.host_key)?);
                }
            }
        }
        Ok(preferred)
    }
}

pub fn catalog() -> serde_json::Value {
    serde_json::json!({
        "profiles": ["modern", "compatible", "legacy", "custom"],
        "kex": [
            "mlkem768x25519-sha256",
            "curve25519-sha256",
            "curve25519-sha256@libssh.org",
            "ecdh-sha2-nistp256",
            "ecdh-sha2-nistp384",
            "ecdh-sha2-nistp521",
            "diffie-hellman-group16-sha512",
            "diffie-hellman-group14-sha256",
            "diffie-hellman-group-exchange-sha256",
            "diffie-hellman-group14-sha1",
            "diffie-hellman-group1-sha1",
            "diffie-hellman-group-exchange-sha1"
        ],
        "cipher": [
            "chacha20-poly1305@openssh.com",
            "aes256-gcm@openssh.com",
            "aes128-gcm@openssh.com",
            "aes256-ctr",
            "aes192-ctr",
            "aes128-ctr",
            "aes256-cbc",
            "aes192-cbc",
            "aes128-cbc",
            "3des-cbc"
        ],
        "mac": [
            "hmac-sha2-256-etm@openssh.com",
            "hmac-sha2-512-etm@openssh.com",
            "hmac-sha2-256",
            "hmac-sha2-512",
            "hmac-sha1-etm@openssh.com",
            "hmac-sha1"
        ],
        "host_key": [
            "ssh-ed25519",
            "ecdsa-sha2-nistp256",
            "ecdsa-sha2-nistp384",
            "ecdsa-sha2-nistp521",
            "rsa-sha2-512",
            "rsa-sha2-256",
            "ssh-rsa"
        ]
    })
}

const COMPAT_KEX: &[kex::Name] = &[
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G16_SHA512,
    kex::DH_G14_SHA256,
    kex::DH_GEX_SHA256,
    kex::DH_G14_SHA1,
];

const LEGACY_KEX: &[kex::Name] = &[
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G16_SHA512,
    kex::DH_G14_SHA256,
    kex::DH_GEX_SHA256,
    kex::DH_G14_SHA1,
    kex::DH_G1_SHA1,
    kex::DH_GEX_SHA1,
];

const COMPAT_CIPHER: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
    cipher::AES_256_CBC,
    cipher::AES_192_CBC,
    cipher::AES_128_CBC,
];

const LEGACY_CIPHER: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
    cipher::AES_256_CBC,
    cipher::AES_192_CBC,
    cipher::AES_128_CBC,
    cipher::TRIPLE_DES_CBC,
];

const COMPAT_MAC: &[mac::Name] = &[
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256,
    mac::HMAC_SHA512,
];

const LEGACY_MAC: &[mac::Name] = &[
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256,
    mac::HMAC_SHA512,
    mac::HMAC_SHA1_ETM,
    mac::HMAC_SHA1,
];

fn compat_host_keys() -> Vec<Algorithm> {
    vec![
        Algorithm::Ed25519,
        Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 },
        Algorithm::Ecdsa { curve: EcdsaCurve::NistP384 },
        Algorithm::Ecdsa { curve: EcdsaCurve::NistP521 },
        Algorithm::Rsa { hash: Some(HashAlg::Sha512) },
        Algorithm::Rsa { hash: Some(HashAlg::Sha256) },
    ]
}

fn legacy_host_keys() -> Vec<Algorithm> {
    let mut keys = compat_host_keys();
    keys.push(Algorithm::Rsa { hash: None });
    keys
}

fn map_kex(names: &[String]) -> Result<Vec<kex::Name>> {
    names
        .iter()
        .map(|n| {
            Ok(match n.as_str() {
                "curve25519-sha256" => kex::CURVE25519,
                "curve25519-sha256@libssh.org" => kex::CURVE25519_PRE_RFC_8731,
                "ecdh-sha2-nistp256" => kex::ECDH_SHA2_NISTP256,
                "ecdh-sha2-nistp384" => kex::ECDH_SHA2_NISTP384,
                "ecdh-sha2-nistp521" => kex::ECDH_SHA2_NISTP521,
                "diffie-hellman-group16-sha512" => kex::DH_G16_SHA512,
                "diffie-hellman-group15-sha512" => kex::DH_G15_SHA512,
                "diffie-hellman-group14-sha256" => kex::DH_G14_SHA256,
                "diffie-hellman-group14-sha1" => kex::DH_G14_SHA1,
                "diffie-hellman-group1-sha1" => kex::DH_G1_SHA1,
                "diffie-hellman-group-exchange-sha256" => kex::DH_GEX_SHA256,
                "diffie-hellman-group-exchange-sha1" => kex::DH_GEX_SHA1,
                other => return Err(Error::msg(format!("unknown kex algorithm: {other}"))),
            })
        })
        .collect()
}

fn map_cipher(names: &[String]) -> Result<Vec<cipher::Name>> {
    names
        .iter()
        .map(|n| {
            Ok(match n.as_str() {
                "chacha20-poly1305@openssh.com" => cipher::CHACHA20_POLY1305,
                "aes256-gcm@openssh.com" => cipher::AES_256_GCM,
                "aes128-gcm@openssh.com" => cipher::AES_128_GCM,
                "aes256-ctr" => cipher::AES_256_CTR,
                "aes192-ctr" => cipher::AES_192_CTR,
                "aes128-ctr" => cipher::AES_128_CTR,
                "aes256-cbc" => cipher::AES_256_CBC,
                "aes192-cbc" => cipher::AES_192_CBC,
                "aes128-cbc" => cipher::AES_128_CBC,
                "3des-cbc" => cipher::TRIPLE_DES_CBC,
                other => return Err(Error::msg(format!("unknown cipher: {other}"))),
            })
        })
        .collect()
}

fn map_mac(names: &[String]) -> Result<Vec<mac::Name>> {
    names
        .iter()
        .map(|n| {
            Ok(match n.as_str() {
                "hmac-sha2-256-etm@openssh.com" => mac::HMAC_SHA256_ETM,
                "hmac-sha2-512-etm@openssh.com" => mac::HMAC_SHA512_ETM,
                "hmac-sha2-256" => mac::HMAC_SHA256,
                "hmac-sha2-512" => mac::HMAC_SHA512,
                "hmac-sha1-etm@openssh.com" => mac::HMAC_SHA1_ETM,
                "hmac-sha1" => mac::HMAC_SHA1,
                other => return Err(Error::msg(format!("unknown mac: {other}"))),
            })
        })
        .collect()
}

fn map_host_key(names: &[String]) -> Result<Vec<Algorithm>> {
    names
        .iter()
        .map(|n| {
            Algorithm::new(n).map_err(|e| Error::msg(format!("unknown host key algorithm {n}: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_does_not_advertise_3des() {
        let preferred = AlgSpec::modern().to_preferred().unwrap();
        assert!(!preferred.cipher.iter().any(|c| c.as_ref() == "3des-cbc"));
    }

    #[test]
    fn legacy_includes_3des_and_sha1() {
        let preferred = AlgSpec::legacy().to_preferred().unwrap();
        assert!(preferred.cipher.iter().any(|c| c.as_ref() == "3des-cbc"));
        assert!(preferred.mac.iter().any(|m| m.as_ref() == "hmac-sha1"));
        assert!(preferred.kex.iter().any(|k| k.as_ref() == "diffie-hellman-group1-sha1"));
    }

    #[test]
    fn custom_whitelist_roundtrip() {
        let spec = AlgSpec {
            profile: AlgProfileKind::Custom,
            cipher: vec!["aes256-ctr".into(), "3des-cbc".into()],
            mac: vec!["hmac-sha1".into()],
            kex: vec!["diffie-hellman-group14-sha1".into()],
            host_key: vec!["ssh-rsa".into()],
        };
        let preferred = spec.to_preferred().unwrap();
        assert_eq!(preferred.cipher.len(), 2);
        assert_eq!(preferred.mac.len(), 1);
    }
}
