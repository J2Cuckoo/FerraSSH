//! 主密码保险库：PBKDF2-HMAC-SM3 → SM4-CBC + HMAC-SM3（国密）。
//! 旧版 Argon2id + AES-256-GCM 仅用于迁移已有 `ferrassh.db`。

use aes_gcm::aead::{Aead, KeyInit as AesKeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use cipher::block_padding::Pkcs7;
use cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sm3::Sm3;
use sm4::Sm4;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{Error, Result};

pub const KDF_ITERS: u32 = 210_000;
pub const KDF_M_KIB: u32 = 64 * 1024;
pub const KDF_T: u32 = 3;
pub const KDF_P: u32 = 4;
const SALT_LEN: usize = 16;
const SM4_KEY_LEN: usize = 16;
const MAC_KEY_LEN: usize = 32;
const OKM_LEN: usize = SM4_KEY_LEN + MAC_KEY_LEN;
const IV_LEN: usize = 16;
const AES_NONCE_LEN: usize = 12;
const AES_KEY_LEN: usize = 32;
const GM_MAGIC: &[u8; 4] = b"GM1\0";
const VERIFIER_PLAIN: &[u8] = b"FERRASSH-VAULT-GM1";
const LEGACY_VERIFIER: &[u8] = b"FERRASSH-VAULT-v1";

type Sm4CbcEnc = cbc::Encryptor<Sm4>;
type Sm4CbcDec = cbc::Decryptor<Sm4>;
type HmacSm3 = Hmac<Sm3>;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey {
    sm4: [u8; SM4_KEY_LEN],
    mac: [u8; MAC_KEY_LEN],
}

impl VaultKey {
    fn from_okm(okm: [u8; OKM_LEN]) -> Self {
        let mut sm4 = [0u8; SM4_KEY_LEN];
        let mut mac = [0u8; MAC_KEY_LEN];
        sm4.copy_from_slice(&okm[..SM4_KEY_LEN]);
        mac.copy_from_slice(&okm[SM4_KEY_LEN..]);
        Self { sm4, mac }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct LegacyAesKey {
    key: [u8; AES_KEY_LEN],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    /// 旧版 Argon2 内存参数；国密 PBKDF2 忽略。
    pub m_kib: u32,
    /// 国密：PBKDF2 迭代次数。旧版：Argon2 time cost。
    pub t: u32,
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self { m_kib: KDF_M_KIB, t: KDF_ITERS, p: KDF_P }
    }
}

pub fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

pub fn derive_key(password: &str, salt: &[u8], params: &KdfParams) -> Result<VaultKey> {
    let iters = params.t.max(1);
    let mut okm = [0u8; OKM_LEN];
    pbkdf2_hmac::<Sm3>(password.as_bytes(), salt, iters, &mut okm);
    Ok(VaultKey::from_okm(okm))
}

pub fn encrypt(key: &VaultKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut iv = [0u8; IV_LEN];
    rand::thread_rng().fill_bytes(&mut iv);
    let ct = Sm4CbcEnc::new((&key.sm4).into(), (&iv).into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext);
    let mut mac = <HmacSm3 as Mac>::new_from_slice(&key.mac).map_err(|e| Error::Kdf(e.to_string()))?;
    mac.update(&iv);
    mac.update(&ct);
    let tag = mac.finalize().into_bytes();
    let mut out = Vec::with_capacity(GM_MAGIC.len() + IV_LEN + ct.len() + tag.len());
    out.extend_from_slice(GM_MAGIC);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ct);
    out.extend_from_slice(&tag);
    Ok(out)
}

pub fn decrypt(key: &VaultKey, blob: &[u8]) -> Result<Vec<u8>> {
    let min = GM_MAGIC.len() + IV_LEN + 16 + 32;
    if blob.len() < min || blob[..4] != *GM_MAGIC {
        return Err(Error::Crypto);
    }
    let iv = &blob[4..4 + IV_LEN];
    let tag = &blob[blob.len() - 32..];
    let ct = &blob[4 + IV_LEN..blob.len() - 32];
    let mut mac = <HmacSm3 as Mac>::new_from_slice(&key.mac).map_err(|e| Error::Kdf(e.to_string()))?;
    mac.update(iv);
    mac.update(ct);
    mac.verify_slice(tag).map_err(|_| Error::Crypto)?;
    let iv_arr: [u8; IV_LEN] = iv.try_into().map_err(|_| Error::Crypto)?;
    Sm4CbcDec::new((&key.sm4).into(), (&iv_arr).into())
        .decrypt_padded_vec_mut::<Pkcs7>(ct)
        .map_err(|_| Error::Crypto)
}

pub fn make_verifier(key: &VaultKey) -> Result<Vec<u8>> {
    encrypt(key, VERIFIER_PLAIN)
}

pub fn verify(key: &VaultKey, verifier: &[u8]) -> Result<()> {
    let plain = decrypt(key, verifier)?;
    if plain.as_slice() == VERIFIER_PLAIN {
        Ok(())
    } else {
        Err(Error::BadPassword)
    }
}

pub fn legacy_derive_key(password: &str, salt: &[u8], params: &KdfParams) -> Result<LegacyAesKey> {
    let argon_params = Params::new(params.m_kib.max(8), params.t.max(1), params.p.max(1), Some(AES_KEY_LEN))
        .map_err(|e| Error::Kdf(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = [0u8; AES_KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Kdf(e.to_string()))?;
    Ok(LegacyAesKey { key })
}

pub fn legacy_decrypt(key: &LegacyAesKey, blob: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < AES_NONCE_LEN + 16 {
        return Err(Error::msg("ciphertext too short"));
    }
    let cipher = Aes256Gcm::new_from_slice(&key.key).map_err(|e| Error::msg(e.to_string()))?;
    let nonce = Nonce::from_slice(&blob[..AES_NONCE_LEN]);
    cipher.decrypt(nonce, &blob[AES_NONCE_LEN..]).map_err(|_| Error::Crypto)
}

pub fn legacy_verify(key: &LegacyAesKey, verifier: &[u8]) -> Result<()> {
    let plain = legacy_decrypt(key, verifier)?;
    if plain.as_slice() == LEGACY_VERIFIER {
        Ok(())
    } else {
        Err(Error::BadPassword)
    }
}

/// Envelope used by the optional zero-knowledge sync channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub revision: u64,
    pub device_id: String,
    pub updated_at: i64,
    pub ciphertext: Vec<u8>,
}

pub const VAULT_MAGIC: &[u8; 4] = b"FSGM";
pub const VAULT_VERSION: u8 = 1;

#[derive(Clone)]
pub struct VaultFileHeader {
    pub params: KdfParams,
    pub salt: Vec<u8>,
    pub verifier: Vec<u8>,
}

pub fn encode_vault_file(header: &VaultFileHeader, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + header.salt.len() + header.verifier.len() + body.len());
    out.extend_from_slice(VAULT_MAGIC);
    out.push(VAULT_VERSION);
    out.extend_from_slice(&header.params.t.to_le_bytes());
    let salt_len = u16::try_from(header.salt.len()).unwrap_or(0);
    out.extend_from_slice(&salt_len.to_le_bytes());
    out.extend_from_slice(&header.salt);
    let ver_len = u32::try_from(header.verifier.len()).unwrap_or(0);
    out.extend_from_slice(&ver_len.to_le_bytes());
    out.extend_from_slice(&header.verifier);
    let body_len = u64::try_from(body.len()).unwrap_or(0);
    out.extend_from_slice(&body_len.to_le_bytes());
    out.extend_from_slice(body);
    out
}

pub fn decode_vault_file(bytes: &[u8]) -> Result<(VaultFileHeader, Vec<u8>)> {
    if bytes.len() < 4 + 1 + 4 + 2 + 4 + 8 || bytes[..4] != *VAULT_MAGIC {
        return Err(Error::msg("invalid vault file"));
    }
    let version = bytes[4];
    if version != VAULT_VERSION {
        return Err(Error::msg(format!("unsupported vault version {version}")));
    }
    let mut i = 5;
    let t = u32::from_le_bytes(bytes[i..i + 4].try_into().map_err(|_| Error::msg("vault header"))?);
    i += 4;
    let salt_len = u16::from_le_bytes(bytes[i..i + 2].try_into().map_err(|_| Error::msg("vault header"))?) as usize;
    i += 2;
    if bytes.len() < i + salt_len + 4 {
        return Err(Error::msg("truncated vault salt"));
    }
    let salt = bytes[i..i + salt_len].to_vec();
    i += salt_len;
    let ver_len = u32::from_le_bytes(bytes[i..i + 4].try_into().map_err(|_| Error::msg("vault header"))?) as usize;
    i += 4;
    if bytes.len() < i + ver_len + 8 {
        return Err(Error::msg("truncated vault verifier"));
    }
    let verifier = bytes[i..i + ver_len].to_vec();
    i += ver_len;
    let body_len = u64::from_le_bytes(bytes[i..i + 8].try_into().map_err(|_| Error::msg("vault header"))?) as usize;
    i += 8;
    if bytes.len() < i + body_len {
        return Err(Error::msg("truncated vault body"));
    }
    let body = bytes[i..i + body_len].to_vec();
    Ok((
        VaultFileHeader {
            params: KdfParams { m_kib: 0, t, p: 1 },
            salt,
            verifier,
        },
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_password() {
        let params = KdfParams { m_kib: 8, t: 2, p: 1 };
        let salt = random_salt();
        let key = derive_key("correct horse", &salt, &params).unwrap();
        let blob = encrypt(&key, b"secret-key-material").unwrap();
        assert_eq!(decrypt(&key, &blob).unwrap(), b"secret-key-material");

        let other = derive_key("wrong", &salt, &params).unwrap();
        assert!(decrypt(&other, &blob).is_err());
    }

    #[test]
    fn verifier_rejects_bad_password() {
        let params = KdfParams { m_kib: 8, t: 2, p: 1 };
        let salt = random_salt();
        let key = derive_key("pw", &salt, &params).unwrap();
        let ver = make_verifier(&key).unwrap();
        verify(&key, &ver).unwrap();
        let other = derive_key("nope", &salt, &params).unwrap();
        assert!(verify(&other, &ver).is_err());
    }

    #[test]
    fn vault_file_roundtrip() {
        let header = VaultFileHeader {
            params: KdfParams { m_kib: 0, t: 2, p: 1 },
            salt: vec![1; 16],
            verifier: vec![9; 8],
        };
        let encoded = encode_vault_file(&header, b"body");
        let (h, body) = decode_vault_file(&encoded).unwrap();
        assert_eq!(h.params.t, 2);
        assert_eq!(body, b"body");
    }
}
