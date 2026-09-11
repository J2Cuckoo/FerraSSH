use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::algs::{AlgProfileKind, AlgSpec};
use crate::crypto::{self, KdfParams, LegacyAesKey, VaultFileHeader, VaultKey};
use crate::{Error, Result};

fn default_ops_kind() -> String {
    "ops".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub sort_order: i64,
    #[serde(default = "default_ops_kind")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterProject {
    pub id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub notes: String,
    pub sort_order: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNode {
    pub project_id: String,
    pub session_id: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethodKind {
    Password,
    Key,
    Agent,
    Keyboard,
}

impl Default for AuthMethodKind {
    fn default() -> Self {
        Self::Password
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSecret {
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub private_key_pem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethodKind,
    pub key_id: Option<String>,
    pub jump_host_id: Option<String>,
    pub algs: AlgSpec,
    pub local_echo: bool,
    pub keepalive: u64,
    pub compression: bool,
    pub term: String,
    pub notes: String,
    pub sort_order: i64,
    pub updated_at: i64,
    pub has_secret: bool,
    #[serde(default = "default_true")]
    pub in_ops: bool,
    #[serde(default)]
    pub sftp_local_path: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedKey {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
}

fn default_theme() -> String {
    "ink".into()
}

fn default_term_fg() -> String {
    "#d6deeb".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub idle_lock_secs: u64,
    pub default_term: String,
    pub font_family: String,
    pub font_size: f32,
    pub scrollback: u32,
    pub parallel_transfers: u32,
    pub chunk_kib: u32,
    pub preserve_perms: bool,
    pub follow_symlinks: bool,
    pub sync_url: Option<String>,
    pub sync_account: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_term_fg")]
    pub term_fg: String,
    #[serde(default = "default_log_days")]
    pub log_retain_days: u32,
    #[serde(default)]
    pub minio_endpoint: String,
    #[serde(default)]
    pub minio_bucket: String,
    #[serde(default)]
    pub minio_access_key: String,
    #[serde(default)]
    pub minio_secret_key: String,
    #[serde(default = "default_minio_region")]
    pub minio_region: String,
    #[serde(default = "default_minio_object")]
    pub minio_object: String,
}

fn default_log_days() -> u32 {
    5
}

fn default_minio_region() -> String {
    "us-east-1".into()
}

fn default_minio_object() -> String {
    "latest.json".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self::sane_default()
    }
}

impl AppSettings {
    pub fn sane_default() -> Self {
        Self {
            idle_lock_secs: 15 * 60,
            default_term: "xterm-256color".into(),
            font_family: "JetBrains Mono, Cascadia Mono, Sarasa Mono SC, Noto Sans Mono CJK SC, Microsoft YaHei, monospace"
                .into(),
            font_size: 14.0,
            scrollback: 10_000,
            parallel_transfers: 4,
            chunk_kib: 512,
            preserve_perms: true,
            follow_symlinks: false,
            sync_url: None,
            sync_account: None,
            theme: default_theme(),
            term_fg: default_term_fg(),
            log_retain_days: default_log_days(),
            minio_endpoint: String::new(),
            minio_bucket: String::new(),
            minio_access_key: String::new(),
            minio_secret_key: String::new(),
            minio_region: default_minio_region(),
            minio_object: default_minio_object(),
        }
    }
}

pub struct Store {
    conn: Mutex<Connection>,
    key: Mutex<Option<Arc<VaultKey>>>,
    header: Mutex<Option<VaultFileHeader>>,
    path: PathBuf,
    legacy_path: PathBuf,
}

const SCHEMA: &str = "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS vault_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                kdf_salt BLOB NOT NULL,
                kdf_m INTEGER NOT NULL,
                kdf_t INTEGER NOT NULL,
                kdf_p INTEGER NOT NULL,
                verifier BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS folders (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL DEFAULT 'ops'
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                folder_id TEXT,
                name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL DEFAULT 22,
                username TEXT NOT NULL,
                auth_method TEXT NOT NULL,
                secret_blob BLOB,
                key_id TEXT,
                jump_host_id TEXT,
                alg_json TEXT NOT NULL,
                local_echo INTEGER NOT NULL DEFAULT 0,
                keepalive INTEGER NOT NULL DEFAULT 30,
                compression INTEGER NOT NULL DEFAULT 0,
                term TEXT NOT NULL DEFAULT 'xterm-256color',
                notes TEXT NOT NULL DEFAULT '',
                sort_order INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                in_ops INTEGER NOT NULL DEFAULT 1,
                sftp_local_path TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                folder_id TEXT,
                name TEXT NOT NULL,
                notes TEXT NOT NULL DEFAULT '',
                sort_order INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS project_nodes (
                project_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (project_id, session_id)
            );
            CREATE TABLE IF NOT EXISTS ssh_keys (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                public_key TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                encrypted_private BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS known_hosts (
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                algorithm TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                PRIMARY KEY (host, port)
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS kb_docs (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sync_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                device_id TEXT NOT NULL,
                remote_url TEXT,
                token_blob BLOB,
                revision INTEGER NOT NULL DEFAULT 0,
                last_sync INTEGER
            );
            ";

impl Store {
    pub fn open_default() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com", "Ferra", "FerraSSH")
            .ok_or_else(|| Error::msg("cannot resolve data directory"))?;
        let dir = dirs.data_dir();
        std::fs::create_dir_all(dir)?;
        Self::open(dir.join("ferrassh.vault"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let legacy_path = path
            .parent()
            .map(|p| p.join("ferrassh.db"))
            .unwrap_or_else(|| PathBuf::from("ferrassh.db"));
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            key: Mutex::new(None),
            header: Mutex::new(None),
            path,
            legacy_path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_initialized(&self) -> Result<bool> {
        Ok(self.path.exists() || self.legacy_path.exists())
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.lock().is_some()
    }

    fn require_key(&self) -> Result<Arc<VaultKey>> {
        self.key.lock().clone().ok_or(Error::VaultLocked)
    }

    fn persist(&self) -> Result<()> {
        let key = self.require_key()?;
        let header = self.header.lock().clone().ok_or(Error::VaultLocked)?;
        let plain = {
            let conn = self.conn.lock();
            conn.serialize(rusqlite::DatabaseName::Main)?.to_vec()
        };
        let body = crypto::encrypt(&key, &plain)?;
        let encoded = crypto::encode_vault_file(&header, &body);
        write_atomic(&self.path, &encoded)
    }

    pub fn initialize(&self, password: &str) -> Result<()> {
        self.initialize_with_params(password, KdfParams::default())
    }

    pub fn initialize_with_params(&self, password: &str, params: KdfParams) -> Result<()> {
        if self.is_initialized()? {
            return Err(Error::msg("vault already initialized"));
        }
        let salt = crypto::random_salt();
        let key = crypto::derive_key(password, &salt, &params)?;
        let verifier = crypto::make_verifier(&key)?;
        let now = now_secs();
        {
            let conn = self.conn.lock();
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO vault_meta (id, kdf_salt, kdf_m, kdf_t, kdf_p, verifier, created_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
                params![salt.as_slice(), params.m_kib as i64, params.t as i64, params.p as i64, verifier, now],
            )?;
            let settings = AppSettings::sane_default();
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('app', ?1)",
                params![serde_json::to_string(&settings)?],
            )?;
        }
        *self.header.lock() = Some(VaultFileHeader {
            params: params.clone(),
            salt: salt.to_vec(),
            verifier,
        });
        *self.key.lock() = Some(Arc::new(key));
        self.persist()
    }

    fn load_vault_meta(conn: &Connection) -> Result<(Vec<u8>, KdfParams, Vec<u8>)> {
        let row = conn
            .query_row(
                "SELECT kdf_salt, kdf_m, kdf_t, kdf_p, verifier FROM vault_meta WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::VaultMissing)?;
        Ok((
            row.0,
            KdfParams { m_kib: row.1 as u32, t: row.2 as u32, p: row.3 as u32 },
            row.4,
        ))
    }

    pub fn unlock(&self, password: &str) -> Result<()> {
        if self.path.exists() {
            return self.unlock_gm(password);
        }
        if self.legacy_path.exists() {
            return self.unlock_and_migrate_legacy(password);
        }
        Err(Error::VaultMissing)
    }

    fn unlock_gm(&self, password: &str) -> Result<()> {
        let raw = std::fs::read(&self.path)?;
        let (header, body) = crypto::decode_vault_file(&raw)?;
        let key = crypto::derive_key(password, &header.salt, &header.params)?;
        crypto::verify(&key, &header.verifier).map_err(|_| Error::BadPassword)?;
        let plain = crypto::decrypt(&key, &body)?;
        let conn = sqlite_from_bytes(&plain)?;
        *self.conn.lock() = conn;
        *self.header.lock() = Some(header);
        *self.key.lock() = Some(Arc::new(key));
        self.ensure_schema()?;
        Ok(())
    }

    fn unlock_and_migrate_legacy(&self, password: &str) -> Result<()> {
        let file_conn = Connection::open(&self.legacy_path)?;
        let (salt, params, verifier) = Self::load_vault_meta(&file_conn)?;
        let aes = crypto::legacy_derive_key(password, &salt, &params)?;
        crypto::legacy_verify(&aes, &verifier).map_err(|_| Error::BadPassword)?;
        *self.conn.lock() = file_conn;

        let session_ids: Vec<String> = self.list_sessions()?.into_iter().map(|s| s.id).collect();
        let mut secrets = Vec::new();
        for id in &session_ids {
            if let Some(secret) = load_legacy_secret(&self.conn.lock(), &aes, id)? {
                secrets.push((id.clone(), secret));
            }
        }
        let key_rows = self.list_keys()?;
        let mut pems = Vec::new();
        for k in &key_rows {
            pems.push((k.id.clone(), load_legacy_private(&self.conn.lock(), &aes, &k.id)?));
        }

        let gm_params = KdfParams::default();
        let gm_salt = crypto::random_salt();
        let gm_key = crypto::derive_key(password, &gm_salt, &gm_params)?;
        let gm_verifier = crypto::make_verifier(&gm_key)?;
        *self.header.lock() = Some(VaultFileHeader {
            params: gm_params.clone(),
            salt: gm_salt.to_vec(),
            verifier: gm_verifier.clone(),
        });
        *self.key.lock() = Some(Arc::new(gm_key));
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE vault_meta SET kdf_salt=?1, kdf_m=?2, kdf_t=?3, kdf_p=?4, verifier=?5 WHERE id=1",
                params![
                    gm_salt.as_slice(),
                    gm_params.m_kib as i64,
                    gm_params.t as i64,
                    gm_params.p as i64,
                    gm_verifier
                ],
            )?;
        }
        for (id, secret) in secrets {
            self.save_secret(&id, &secret)?;
        }
        for (id, pem) in pems {
            self.update_private_key_blob(&id, &pem)?;
        }

        let mem = {
            let conn = self.conn.lock();
            sqlite_from_bytes(&conn.serialize(rusqlite::DatabaseName::Main)?.to_vec())?
        };
        *self.conn.lock() = mem;
        self.persist()?;
        let bak = self.legacy_path.with_extension("db.pre-gm");
        let _ = std::fs::rename(&self.legacy_path, &bak);
        self.ensure_schema()?;
        Ok(())
    }

    /// Confirm the master password without changing unlock state.
    pub fn verify_password(&self, password: &str) -> Result<()> {
        let header = if let Some(h) = self.header.lock().clone() {
            h
        } else if self.path.exists() {
            crypto::decode_vault_file(&std::fs::read(&self.path)?)?.0
        } else {
            return Err(Error::VaultMissing);
        };
        let key = crypto::derive_key(password, &header.salt, &header.params)?;
        crypto::verify(&key, &header.verifier).map_err(|_| Error::BadPassword)
    }

    pub fn lock(&self) {
        let _ = self.persist();
        *self.key.lock() = None;
        if let Ok(conn) = Connection::open_in_memory() {
            let _ = conn.execute_batch(SCHEMA);
            *self.conn.lock() = conn;
        }
    }

    pub fn change_password(&self, old: &str, new: &str) -> Result<()> {
        self.unlock(old)?;
        let params = KdfParams::default();
        let salt = crypto::random_salt();
        let new_key = crypto::derive_key(new, &salt, &params)?;
        let sessions = self.list_sessions()?;
        let mut secrets = Vec::new();
        for s in &sessions {
            secrets.push((s.id.clone(), self.load_secret(&s.id)?));
        }
        let keys = self.list_keys()?;
        let mut private_keys = Vec::new();
        for k in &keys {
            private_keys.push((k.id.clone(), self.load_private_key(&k.id)?));
        }
        let verifier = crypto::make_verifier(&new_key)?;
        {
            let conn = self.conn.lock();
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE vault_meta SET kdf_salt=?1, kdf_m=?2, kdf_t=?3, kdf_p=?4, verifier=?5 WHERE id=1",
                params![salt.as_slice(), params.m_kib as i64, params.t as i64, params.p as i64, verifier],
            )?;
            tx.commit()?;
        }
        *self.header.lock() = Some(VaultFileHeader {
            params: params.clone(),
            salt: salt.to_vec(),
            verifier,
        });
        *self.key.lock() = Some(Arc::new(new_key));
        for (id, secret) in secrets {
            if let Some(secret) = secret {
                self.save_secret(&id, &secret)?;
            }
        }
        for (id, pem) in private_keys {
            self.update_private_key_blob(&id, &pem)?;
        }
        self.persist()
    }

    pub fn settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key='app'", [], |r| r.get(0))
            .optional()?;
        Ok(raw
            .map(|s| serde_json::from_str(&s).unwrap_or_else(|_| AppSettings::sane_default()))
            .unwrap_or_else(AppSettings::sane_default))
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('app', ?1)",
            params![serde_json::to_string(settings)?],
        )?;
        crate::oplog::prune_async(settings.log_retain_days);
        self.persist()
    }

    pub fn ensure_schema(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS kb_docs (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                folder_id TEXT,
                name TEXT NOT NULL,
                notes TEXT NOT NULL DEFAULT '',
                sort_order INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS project_nodes (
                project_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (project_id, session_id)
            );
            "#,
        )?;
        add_column_if_missing(&conn, "folders", "kind", "TEXT NOT NULL DEFAULT 'ops'")?;
        add_column_if_missing(&conn, "sessions", "in_ops", "INTEGER NOT NULL DEFAULT 1")?;
        add_column_if_missing(&conn, "sessions", "sftp_local_path", "TEXT NOT NULL DEFAULT ''")?;
        Ok(())
    }

    pub fn ai_settings(&self) -> Result<crate::ai::AiSettings> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key='ai'", [], |r| r.get(0))
            .optional()?;
        Ok(raw
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default())
    }

    pub fn save_ai_settings(&self, settings: &crate::ai::AiSettings) -> Result<()> {
        let _ = self.require_key()?;
        let settings = settings.sanitized();
        settings.validate_for_enable()?;
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('ai', ?1)",
            params![serde_json::to_string(&settings)?],
        )?;
        self.persist()
    }

    pub fn import_kb_json(&self, source: &str, raw: &str) -> Result<u32> {
        let _ = self.require_key()?;
        self.ensure_schema()?;
        let docs = crate::ai::parse_kb_json(source, raw)?;
        let now = now_secs();
        let n = docs.len() as u32;
        {
            let conn = self.conn.lock();
            for (title, body, tags) in docs {
                conn.execute(
                    "INSERT INTO kb_docs (id, source, title, body, tags, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![Uuid::new_v4().to_string(), source, title, body, tags, now],
                )?;
            }
        }
        self.persist()?;
        Ok(n)
    }

    pub fn list_kb(&self) -> Result<Vec<crate::ai::KbDoc>> {
        self.ensure_schema()?;
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, source, title, body, tags, created_at FROM kb_docs ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::ai::KbDoc {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                tags: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn kb_sources(&self) -> Result<Vec<crate::ai::KbSourceSummary>> {
        self.ensure_schema()?;
        let docs = self.list_kb()?;
        let mut order: Vec<String> = Vec::new();
        let mut map: std::collections::BTreeMap<String, crate::ai::KbSourceSummary> = std::collections::BTreeMap::new();
        for d in docs {
            let e = map.entry(d.source.clone()).or_insert_with(|| {
                order.push(d.source.clone());
                crate::ai::KbSourceSummary {
                    source: d.source.clone(),
                    count: 0,
                    titles: Vec::new(),
                }
            });
            e.count += 1;
            if e.titles.len() < 4 {
                e.titles.push(d.title);
            }
        }
        Ok(order
            .into_iter()
            .filter_map(|s| map.remove(&s))
            .collect())
    }

    pub fn list_kb_by_source(&self, source: &str) -> Result<Vec<crate::ai::KbDoc>> {
        Ok(self.list_kb()?.into_iter().filter(|d| d.source == source).collect())
    }

    pub fn save_kb_docs(&self, old_source: &str, new_source: &str, docs: &[crate::ai::KbDoc]) -> Result<()> {
        let _ = self.require_key()?;
        self.ensure_schema()?;
        let new_source = new_source.trim();
        if new_source.is_empty() {
            return Err(Error::msg("知识库名称不能为空"));
        }
        if docs.is_empty() || docs.iter().all(|d| d.title.trim().is_empty() && d.body.trim().is_empty()) {
            return Err(Error::msg("知识库至少保留一条内容"));
        }
        if new_source != old_source {
            let clash = self.kb_sources()?.iter().any(|s| s.source == new_source);
            if clash {
                return Err(Error::msg("已存在同名知识库，请换一个名称"));
            }
        }
        let now = now_secs();
        {
            let conn = self.conn.lock();
            conn.execute("DELETE FROM kb_docs WHERE source=?1", params![old_source])?;
            for d in docs {
                if d.title.trim().is_empty() && d.body.trim().is_empty() {
                    continue;
                }
                let id = if d.id.trim().is_empty() {
                    Uuid::new_v4().to_string()
                } else {
                    d.id.clone()
                };
                conn.execute(
                    "INSERT INTO kb_docs (id, source, title, body, tags, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        id,
                        new_source,
                        d.title.trim(),
                        d.body.trim(),
                        d.tags.trim(),
                        if d.created_at > 0 { d.created_at } else { now }
                    ],
                )?;
            }
        }
        self.persist()
    }

    pub fn delete_kb_source(&self, source: &str) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute("DELETE FROM kb_docs WHERE source=?1", params![source])?;
        self.persist()
    }

    pub fn clear_kb(&self) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute("DELETE FROM kb_docs", [])?;
        self.persist()
    }

    pub fn search_kb(&self, query: &str, extra: &str, limit: usize) -> Result<Vec<crate::ai::KbDoc>> {
        self.ensure_schema()?;
        let mut tokens: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_')
            .chain(extra.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_'))
            .map(|s| s.trim().to_lowercase())
            .filter(|s| s.len() >= 2)
            .collect();
        tokens.sort();
        tokens.dedup();
        if tokens.is_empty() {
            tokens.push("%".into());
        }
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, source, title, body, tags, created_at FROM kb_docs")?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::ai::KbDoc {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                tags: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        let mut scored: Vec<(i32, crate::ai::KbDoc)> = Vec::new();
        for doc in rows.filter_map(|r| r.ok()) {
            let hay = format!("{} {} {} {}", doc.title, doc.body, doc.tags, doc.source).to_lowercase();
            let mut score = 0i32;
            for t in &tokens {
                if t == "%" {
                    continue;
                }
                if hay.contains(t) {
                    score += 2;
                    if doc.title.to_lowercase().contains(t) {
                        score += 3;
                    }
                }
            }
            if score > 0 || tokens.iter().any(|t| t == "%") {
                if tokens.iter().any(|t| t == "%") {
                    score = score.max(1);
                }
                scored.push((score, doc));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(scored.into_iter().take(limit).map(|(_, d)| d).collect())
    }

    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, parent_id, name, sort_order, COALESCE(kind, 'ops') FROM folders ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                name: r.get(2)?,
                sort_order: r.get(3)?,
                kind: r.get::<_, String>(4).unwrap_or_else(|_| "ops".into()),
            })
        })?;
        Ok(rows.flatten().collect())
    }

    pub fn upsert_folder(&self, folder: &Folder) -> Result<()> {
        let _ = self.require_key()?;
        let kind = if folder.kind.trim().is_empty() { "ops" } else { folder.kind.trim() };
        self.conn.lock().execute(
            "INSERT INTO folders (id, parent_id, name, sort_order, kind) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id, name=excluded.name, sort_order=excluded.sort_order, kind=excluded.kind",
            params![folder.id, folder.parent_id, folder.name, folder.sort_order, kind],
        )?;
        self.persist()
    }

    pub fn delete_folder(&self, id: &str) -> Result<()> {
        let _ = self.require_key()?;
        let conn = self.conn.lock();
        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM folders WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        conn.execute(
            "UPDATE folders SET parent_id=?1 WHERE parent_id=?2",
            params![parent, id],
        )?;
        conn.execute("UPDATE sessions SET folder_id=NULL WHERE folder_id=?1", params![id])?;
        conn.execute("UPDATE projects SET folder_id=?1 WHERE folder_id=?2", params![parent, id])?;
        conn.execute("DELETE FROM folders WHERE id=?1", params![id])?;
        drop(conn);
        self.persist()
    }

    pub fn list_sessions(&self) -> Result<Vec<SavedSession>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, folder_id, name, host, port, username, auth_method, secret_blob, key_id,
                    jump_host_id, alg_json, local_echo, keepalive, compression, term, notes,
                    sort_order, updated_at, COALESCE(in_ops, 1), COALESCE(sftp_local_path, '')
             FROM sessions ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |r| {
            let blob: Option<Vec<u8>> = r.get(7)?;
            let alg_json: String = r.get(10)?;
            let algs = serde_json::from_str(&alg_json).unwrap_or_else(|_| AlgSpec {
                profile: AlgProfileKind::Modern,
                ..AlgSpec::default()
            });
            Ok(SavedSession {
                id: r.get(0)?,
                folder_id: r.get(1)?,
                name: r.get(2)?,
                host: r.get(3)?,
                port: r.get::<_, i64>(4)? as u16,
                username: r.get(5)?,
                auth_method: match r.get::<_, String>(6)?.as_str() {
                    "key" => AuthMethodKind::Key,
                    "agent" => AuthMethodKind::Agent,
                    "keyboard" => AuthMethodKind::Keyboard,
                    _ => AuthMethodKind::Password,
                },
                key_id: r.get(8)?,
                jump_host_id: r.get(9)?,
                algs,
                local_echo: r.get::<_, i64>(11)? != 0,
                keepalive: r.get::<_, i64>(12)? as u64,
                compression: r.get::<_, i64>(13)? != 0,
                term: r.get(14)?,
                notes: r.get(15)?,
                sort_order: r.get(16)?,
                updated_at: r.get(17)?,
                has_secret: blob.is_some(),
                in_ops: r.get::<_, i64>(18)? != 0,
                sftp_local_path: r.get::<_, String>(19).unwrap_or_default(),
            })
        })?;
        Ok(rows.flatten().collect())
    }

    pub fn upsert_session(&self, session: &SavedSession, secret: Option<&SessionSecret>) -> Result<()> {
        let _ = self.require_key()?;
        let alg_json = serde_json::to_string(&session.algs)?;
        let auth = match session.auth_method {
            AuthMethodKind::Password => "password",
            AuthMethodKind::Key => "key",
            AuthMethodKind::Agent => "agent",
            AuthMethodKind::Keyboard => "keyboard",
        };
        self.conn.lock().execute(
            "INSERT INTO sessions (id, folder_id, name, host, port, username, auth_method, secret_blob,
                key_id, jump_host_id, alg_json, local_echo, keepalive, compression, term, notes, sort_order, updated_at,
                in_ops, sftp_local_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
             ON CONFLICT(id) DO UPDATE SET
                folder_id=excluded.folder_id, name=excluded.name, host=excluded.host, port=excluded.port,
                username=excluded.username, auth_method=excluded.auth_method, key_id=excluded.key_id,
                jump_host_id=excluded.jump_host_id, alg_json=excluded.alg_json, local_echo=excluded.local_echo,
                keepalive=excluded.keepalive, compression=excluded.compression, term=excluded.term,
                notes=excluded.notes, sort_order=excluded.sort_order, updated_at=excluded.updated_at,
                in_ops=excluded.in_ops, sftp_local_path=excluded.sftp_local_path",
            params![
                session.id,
                session.folder_id,
                session.name,
                session.host,
                session.port as i64,
                session.username,
                auth,
                session.key_id,
                session.jump_host_id,
                alg_json,
                session.local_echo as i64,
                session.keepalive as i64,
                session.compression as i64,
                session.term,
                session.notes,
                session.sort_order,
                now_secs(),
                if session.in_ops { 1 } else { 0 },
                session.sftp_local_path,
            ],
        )?;
        if let Some(secret) = secret {
            self.save_secret(&session.id, secret)?;
        }
        self.persist()
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute("DELETE FROM project_nodes WHERE session_id=?1", params![id])?;
        self.conn.lock().execute("DELETE FROM sessions WHERE id=?1", params![id])?;
        self.persist()
    }

    pub fn list_projects(&self) -> Result<Vec<ClusterProject>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, folder_id, name, notes, sort_order, updated_at FROM projects ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ClusterProject {
                id: r.get(0)?,
                folder_id: r.get(1)?,
                name: r.get(2)?,
                notes: r.get(3)?,
                sort_order: r.get(4)?,
                updated_at: r.get(5)?,
            })
        })?;
        Ok(rows.flatten().collect())
    }

    pub fn upsert_project(&self, project: &ClusterProject) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute(
            "INSERT INTO projects (id, folder_id, name, notes, sort_order, updated_at) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET folder_id=excluded.folder_id, name=excluded.name, notes=excluded.notes,
                sort_order=excluded.sort_order, updated_at=excluded.updated_at",
            params![
                project.id,
                project.folder_id,
                project.name,
                project.notes,
                project.sort_order,
                now_secs()
            ],
        )?;
        self.persist()
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        let _ = self.require_key()?;
        {
            let conn = self.conn.lock();
            conn.execute("DELETE FROM project_nodes WHERE project_id=?1", params![id])?;
            conn.execute("DELETE FROM projects WHERE id=?1", params![id])?;
        }
        self.persist()
    }

    pub fn list_project_nodes(&self) -> Result<Vec<ProjectNode>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT project_id, session_id, sort_order FROM project_nodes ORDER BY sort_order",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ProjectNode {
                project_id: r.get(0)?,
                session_id: r.get(1)?,
                sort_order: r.get(2)?,
            })
        })?;
        Ok(rows.flatten().collect())
    }

    pub fn add_project_node(&self, project_id: &str, session_id: &str, sort_order: i64) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO project_nodes (project_id, session_id, sort_order) VALUES (?1,?2,?3)",
            params![project_id, session_id, sort_order],
        )?;
        self.persist()
    }

    pub fn remove_project_node(&self, project_id: &str, session_id: &str) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute(
            "DELETE FROM project_nodes WHERE project_id=?1 AND session_id=?2",
            params![project_id, session_id],
        )?;
        self.persist()
    }

    pub fn save_secret(&self, session_id: &str, secret: &SessionSecret) -> Result<()> {
        let key = self.require_key()?;
        let blob = crypto::encrypt(&key, &serde_json::to_vec(secret)?)?;
        self.conn.lock().execute(
            "UPDATE sessions SET secret_blob=?1 WHERE id=?2",
            params![blob, session_id],
        )?;
        Ok(())
    }

    pub fn load_secret(&self, session_id: &str) -> Result<Option<SessionSecret>> {
        let key = self.require_key()?;
        let blob: Option<Vec<u8>> = self.conn.lock().query_row(
            "SELECT secret_blob FROM sessions WHERE id=?1",
            params![session_id],
            |r| r.get(0),
        )?;
        match blob {
            Some(blob) => {
                let plain = crypto::decrypt(&key, &blob)?;
                Ok(Some(serde_json::from_slice(&plain)?))
            }
            None => Ok(None),
        }
    }

    pub fn list_keys(&self) -> Result<Vec<SavedKey>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, name, public_key, fingerprint, created_at FROM ssh_keys ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(SavedKey {
                id: r.get(0)?,
                name: r.get(1)?,
                public_key: r.get(2)?,
                fingerprint: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.flatten().collect())
    }

    pub fn generate_ed25519(&self, name: &str) -> Result<SavedKey> {
        let pair = russh::keys::PrivateKey::random(&mut rand::thread_rng(), russh::keys::Algorithm::Ed25519)
            .map_err(|e| Error::msg(format!("generate key: {e}")))?;
        let pem = pair
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .map_err(|e| Error::msg(e.to_string()))?
            .to_string();
        self.import_key(name, &pem)
    }

    pub fn import_key(&self, name: &str, pem: &str) -> Result<SavedKey> {
        let key = self.require_key()?;
        let pair = russh::keys::decode_secret_key(pem, None)
            .map_err(|e| Error::msg(format!("invalid private key: {e}")))?;
        let public = pair
            .public_key()
            .to_openssh()
            .map_err(|e| Error::msg(e.to_string()))?;
        let fingerprint = format!("{}", pair.public_key().fingerprint(russh::keys::HashAlg::Sha256));
        let id = Uuid::new_v4().to_string();
        let blob = crypto::encrypt(&key, pem.as_bytes())?;
        let created = now_secs();
        self.conn.lock().execute(
            "INSERT INTO ssh_keys (id, name, public_key, fingerprint, encrypted_private, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, name, public.clone(), fingerprint.clone(), blob, created],
        )?;
        self.persist()?;
        Ok(SavedKey { id, name: name.into(), public_key: public, fingerprint, created_at: created })
    }

    pub fn load_private_key(&self, key_id: &str) -> Result<String> {
        let vault = self.require_key()?;
        let blob: Vec<u8> = self.conn.lock().query_row(
            "SELECT encrypted_private FROM ssh_keys WHERE id=?1",
            params![key_id],
            |r| r.get(0),
        )?;
        Ok(String::from_utf8(crypto::decrypt(&vault, &blob)?).map_err(|e| Error::msg(e.to_string()))?)
    }

    fn update_private_key_blob(&self, key_id: &str, pem: &str) -> Result<()> {
        let vault = self.require_key()?;
        let blob = crypto::encrypt(&vault, pem.as_bytes())?;
        self.conn.lock().execute(
            "UPDATE ssh_keys SET encrypted_private=?1 WHERE id=?2",
            params![blob, key_id],
        )?;
        Ok(())
    }

    pub fn delete_key(&self, id: &str) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute("DELETE FROM ssh_keys WHERE id=?1", params![id])?;
        self.persist()
    }

    pub fn export_keys_bundle(&self) -> Result<Vec<u8>> {
        let key = self.require_key()?;
        let keys: Vec<(SavedKey, String)> = self
            .list_keys()?
            .into_iter()
            .filter_map(|k| self.load_private_key(&k.id).ok().map(|pem| (k, pem)))
            .collect();
        crypto::encrypt(&key, &serde_json::to_vec(&keys)?)
    }

    pub fn import_keys_bundle(&self, blob: &[u8]) -> Result<u32> {
        let key = self.require_key()?;
        let keys: Vec<(SavedKey, String)> = serde_json::from_slice(&crypto::decrypt(&key, blob)?)
            .map_err(|_| Error::msg("invalid key bundle"))?;
        let mut imported = 0u32;
        for (meta, pem) in keys {
            if self.list_keys()?.iter().any(|k| k.fingerprint == meta.fingerprint) {
                continue;
            }
            self.import_key(&meta.name, &pem)?;
            imported += 1;
        }
        Ok(imported)
    }

    pub fn import_keys_from_file(&self, path: &Path) -> Result<u32> {
        let blob = std::fs::read(path)?;
        if let Ok(text) = std::str::from_utf8(&blob) {
            let trimmed = text.trim();
            if trimmed.contains("BEGIN")
                && (trimmed.contains("PRIVATE KEY") || trimmed.contains("OPENSSH PRIVATE"))
            {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("imported");
                self.import_key(name, trimmed)?;
                return Ok(1);
            }
        }
        self.import_keys_bundle(&blob)
    }

    pub fn get_known_host(&self, host: &str, port: u16) -> Result<Option<KnownHost>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT host, port, algorithm, fingerprint FROM known_hosts WHERE host=?1 AND port=?2",
                params![host, port as i64],
                |r| {
                    Ok(KnownHost {
                        host: r.get(0)?,
                        port: r.get::<_, i64>(1)? as u16,
                        algorithm: r.get(2)?,
                        fingerprint: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn trust_host(&self, host: KnownHost) -> Result<()> {
        let _ = self.require_key()?;
        self.conn.lock().execute(
            "INSERT INTO known_hosts (host, port, algorithm, fingerprint) VALUES (?1,?2,?3,?4)
             ON CONFLICT(host, port) DO UPDATE SET algorithm=excluded.algorithm, fingerprint=excluded.fingerprint",
            params![host.host, host.port as i64, host.algorithm, host.fingerprint],
        )?;
        self.persist()
    }

    pub fn export_vault_snapshot(&self) -> Result<Vec<u8>> {
        let key = self.require_key()?;
        let known = {
            let conn = self.conn.lock();
            let mut stmt = conn.prepare("SELECT host, port, algorithm, fingerprint FROM known_hosts")?;
            let rows = stmt.query_map([], |r| {
                Ok(KnownHost {
                    host: r.get(0)?,
                    port: r.get::<_, i64>(1)? as u16,
                    algorithm: r.get(2)?,
                    fingerprint: r.get(3)?,
                })
            })?;
            rows.flatten().collect::<Vec<_>>()
        };
        let secrets: Vec<(String, SessionSecret)> = self
            .list_sessions()?
            .into_iter()
            .filter_map(|s| self.load_secret(&s.id).ok().flatten().map(|sec| (s.id, sec)))
            .collect();
        let keys: Vec<(SavedKey, String)> = self
            .list_keys()?
            .into_iter()
            .filter_map(|k| self.load_private_key(&k.id).ok().map(|pem| (k, pem)))
            .collect();
        let payload = serde_json::json!({
            "folders": self.list_folders()?,
            "sessions": self.list_sessions()?,
            "secrets": secrets,
            "keys": keys,
            "known_hosts": known,
            "settings": self.settings()?,
        });
        crypto::encrypt(&key, &serde_json::to_vec(&payload)?)
    }

    pub fn import_vault_snapshot(&self, blob: &[u8]) -> Result<()> {
        let key = self.require_key()?;
        let payload: serde_json::Value = serde_json::from_slice(&crypto::decrypt(&key, blob)?)?;
        if let Some(v) = payload.get("folders") {
            let folders: Vec<Folder> = serde_json::from_value(v.clone())?;
            for f in folders {
                self.upsert_folder(&f)?;
            }
        }
        if let Some(v) = payload.get("sessions") {
            let sessions: Vec<SavedSession> = serde_json::from_value(v.clone())?;
            for s in sessions {
                self.upsert_session(&s, None)?;
            }
        }
        if let Some(v) = payload.get("secrets") {
            let secrets: Vec<(String, SessionSecret)> = serde_json::from_value(v.clone())?;
            for (id, secret) in secrets {
                self.save_secret(&id, &secret)?;
            }
        }
        if let Some(v) = payload.get("keys") {
            let keys: Vec<(SavedKey, String)> = serde_json::from_value(v.clone())?;
            for (meta, pem) in keys {
                if self.list_keys()?.iter().any(|k| k.fingerprint == meta.fingerprint) {
                    continue;
                }
                let _ = self.import_key(&meta.name, &pem);
            }
        }
        if let Some(v) = payload.get("known_hosts") {
            let hosts: Vec<KnownHost> = serde_json::from_value(v.clone())?;
            for h in hosts {
                self.trust_host(h)?;
            }
        }
        if let Some(v) = payload.get("settings") {
            let settings: AppSettings = serde_json::from_value(v.clone())?;
            self.save_settings(&settings)?;
        }
        self.persist()
    }
}

fn add_column_if_missing(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .flatten()
        .any(|name| name == column);
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
    }
    Ok(())
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("vault.tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn sqlite_from_bytes(bytes: &[u8]) -> Result<Connection> {
    let tmp = std::env::temp_dir().join(format!("ferrassh-open-{}.db", Uuid::new_v4()));
    std::fs::write(&tmp, bytes)?;
    let src = Connection::open(&tmp)?;
    let mut dst = Connection::open_in_memory()?;
    {
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        backup
            .run_to_completion(64, std::time::Duration::from_millis(0), None)
            .map_err(|e| Error::msg(e.to_string()))?;
    }
    drop(src);
    let _ = std::fs::remove_file(&tmp);
    dst.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(dst)
}

fn load_legacy_secret(conn: &Connection, key: &LegacyAesKey, session_id: &str) -> Result<Option<SessionSecret>> {
    let blob: Option<Vec<u8>> = conn.query_row(
        "SELECT secret_blob FROM sessions WHERE id=?1",
        params![session_id],
        |r| r.get(0),
    )?;
    match blob {
        Some(blob) => {
            let plain = crypto::legacy_decrypt(key, &blob)?;
            Ok(Some(serde_json::from_slice(&plain)?))
        }
        None => Ok(None),
    }
}

fn load_legacy_private(conn: &Connection, key: &LegacyAesKey, key_id: &str) -> Result<String> {
    let blob: Vec<u8> = conn.query_row(
        "SELECT encrypted_private FROM ssh_keys WHERE id=?1",
        params![key_id],
        |r| r.get(0),
    )?;
    String::from_utf8(crypto::legacy_decrypt(key, &blob)?).map_err(|e| Error::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn vault_init_and_session_secret() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path().join("t.vault")).unwrap();
        store
            .initialize_with_params("master-secret", KdfParams { m_kib: 8, t: 2, p: 1 })
            .unwrap();
        let on_disk = std::fs::read(dir.path().join("t.vault")).unwrap();
        assert_eq!(&on_disk[..4], b"FSGM");
        assert!(!on_disk.starts_with(b"SQLite format 3"));
        let session = SavedSession {
            id: "s1".into(),
            folder_id: None,
            name: "lab".into(),
            host: "10.0.0.1".into(),
            port: 22,
            username: "root".into(),
            auth_method: AuthMethodKind::Password,
            key_id: None,
            jump_host_id: None,
            algs: AlgSpec::modern(),
            local_echo: false,
            keepalive: 30,
            compression: false,
            term: "xterm-256color".into(),
            notes: String::new(),
            sort_order: 0,
            updated_at: 0,
            has_secret: false,
            in_ops: true,
            sftp_local_path: String::new(),
        };
        store
            .upsert_session(
                &session,
                Some(&SessionSecret { password: Some("hunter2".into()), passphrase: None, private_key_pem: None }),
            )
            .unwrap();
        store.lock();
        assert!(store.load_secret("s1").is_err());
        store.unlock("master-secret").unwrap();
        let secret = store.load_secret("s1").unwrap().unwrap();
        assert_eq!(secret.password.as_deref(), Some("hunter2"));
        let key = store.generate_ed25519("lab-ed25519").unwrap();
        assert!(key.public_key.contains("ssh-ed25519"));
        assert!(key.fingerprint.contains("SHA256"));
        store.verify_password("master-secret").unwrap();
        assert!(store.verify_password("wrong").is_err());
        let bundle = store.export_keys_bundle().unwrap();
        store.delete_key(&key.id).unwrap();
        assert!(store.list_keys().unwrap().is_empty());
        assert_eq!(store.import_keys_bundle(&bundle).unwrap(), 1);
        assert_eq!(store.list_keys().unwrap().len(), 1);
    }
}
