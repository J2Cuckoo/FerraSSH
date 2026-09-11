mod ai;
mod device_seal;
mod update;

use std::path::PathBuf;
use std::sync::Arc;

use ferra_core::algs;
use ferra_core::session::{ConnectQuick, FrameSink, SessionManager};
use ferra_core::sftp::{self, TransferOptions};
use ferra_core::ssh::ConnectOpts;
use ferra_core::store::{
    AppSettings, AuthMethodKind, ClusterProject, Folder, ProjectNode, SavedSession, SessionSecret, Store,
};
use ferra_core::term::TermFrame;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub store: Arc<Store>,
    pub sessions: Arc<SessionManager>,
    pub ai: Arc<ai::AiHub>,
}

struct TauriSink {
    app: AppHandle,
}

impl FrameSink for TauriSink {
    fn on_frame(&self, session_id: &str, frame: TermFrame) {
        let _ = self.app.emit("term-frame", FramePayload { session_id: session_id.into(), frame });
    }
    fn on_clipboard(&self, session_id: &str, text: String) {
        let _ = self.app.emit("term-clipboard", ClipPayload { session_id: session_id.into(), text });
    }
    fn on_closed(&self, session_id: &str, message: String) {
        let _ = self.app.emit("term-closed", ClosePayload { session_id: session_id.into(), message });
    }
}

#[derive(Clone, Serialize)]
struct FramePayload {
    session_id: String,
    frame: TermFrame,
}
#[derive(Clone, Serialize)]
struct ClipPayload {
    session_id: String,
    text: String,
}
#[derive(Clone, Serialize)]
struct ClosePayload {
    session_id: String,
    message: String,
}

pub(crate) fn map_err(e: ferra_core::Error) -> String {
    e.to_ui()
}

#[tauri::command]
fn vault_status(state: State<AppState>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "initialized": state.store.is_initialized().map_err(map_err)?,
        "unlocked": state.store.is_unlocked(),
    }))
}

#[tauri::command]
async fn vault_init(state: State<'_, AppState>, password: String) -> Result<(), String> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.initialize(&password).map_err(map_err))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn vault_unlock(state: State<'_, AppState>, password: String) -> Result<(), String> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.unlock(&password).map_err(map_err))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn vault_lock(state: State<AppState>) -> Result<(), String> {
    state.store.lock();
    Ok(())
}

#[tauri::command]
async fn vault_change_password(state: State<'_, AppState>, old: String, new: String) -> Result<(), String> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.change_password(&old, &new).map_err(map_err))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_folders(state: State<AppState>) -> Result<Vec<Folder>, String> {
    state.store.list_folders().map_err(map_err)
}

#[tauri::command]
fn upsert_folder(state: State<AppState>, folder: Folder) -> Result<(), String> {
    state.store.upsert_folder(&folder).map_err(map_err)
}

#[tauri::command]
fn delete_folder(state: State<AppState>, id: String) -> Result<(), String> {
    state.store.delete_folder(&id).map_err(map_err)
}

#[tauri::command]
fn list_sessions(state: State<AppState>) -> Result<Vec<SavedSession>, String> {
    state.store.list_sessions().map_err(map_err)
}

#[tauri::command]
fn upsert_session(
    state: State<AppState>,
    session: SavedSession,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<(), String> {
    let secret = if password.is_some() || passphrase.is_some() {
        Some(SessionSecret { password, passphrase, private_key_pem: None })
    } else {
        None
    };
    state.store.upsert_session(&session, secret.as_ref()).map_err(map_err)
}

#[tauri::command]
fn delete_session(state: State<AppState>, id: String) -> Result<(), String> {
    ferra_core::oplog::write("human", "session-delete", &id);
    state.store.delete_session(&id).map_err(map_err)
}

#[tauri::command]
fn list_projects(state: State<AppState>) -> Result<Vec<ClusterProject>, String> {
    state.store.list_projects().map_err(map_err)
}

#[tauri::command]
fn upsert_project(state: State<AppState>, project: ClusterProject) -> Result<(), String> {
    ferra_core::oplog::write("human", "project-save", &project.name);
    state.store.upsert_project(&project).map_err(map_err)
}

#[tauri::command]
fn delete_project(state: State<AppState>, id: String) -> Result<(), String> {
    ferra_core::oplog::write("human", "project-delete", &id);
    state.store.delete_project(&id).map_err(map_err)
}

#[tauri::command]
fn list_project_nodes(state: State<AppState>) -> Result<Vec<ProjectNode>, String> {
    state.store.list_project_nodes().map_err(map_err)
}

#[tauri::command]
fn add_project_node(state: State<AppState>, project_id: String, session_id: String, sort_order: i64) -> Result<(), String> {
    ferra_core::oplog::write("human", "project-node-add", &format!("{project_id} {session_id}"));
    state.store.add_project_node(&project_id, &session_id, sort_order).map_err(map_err)
}

#[tauri::command]
fn remove_project_node(state: State<AppState>, project_id: String, session_id: String) -> Result<(), String> {
    ferra_core::oplog::write("human", "project-node-remove", &format!("{project_id} {session_id}"));
    state.store.remove_project_node(&project_id, &session_id).map_err(map_err)
}

#[tauri::command]
fn oplog_write(actor: String, action: String, detail: String) -> Result<(), String> {
    ferra_core::oplog::write(&actor, &action, &detail);
    Ok(())
}

#[tauri::command]
fn oplog_dir() -> String {
    ferra_core::oplog::logs_dir_display()
}

#[tauri::command]
async fn check_app_update(current: String) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let info = update::check_latest(&client, &current).await?;
    Ok(serde_json::json!({
        "current": info.current,
        "latest": info.latest,
        "newer": info.newer,
        "notes": info.notes
    }))
}

#[tauri::command]
async fn install_app_update(app: AppHandle, current: String) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let info = update::check_latest(&client, &current).await?;
    if !info.newer {
        return Err("当前已是最新版本".into());
    }
    let path = update::download_installer(&app, &info.url).await?;
    update::launch_silent_install(&path)?;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn list_keys(state: State<AppState>) -> Result<Vec<ferra_core::store::SavedKey>, String> {
    state.store.list_keys().map_err(map_err)
}

#[tauri::command]
fn import_key(state: State<AppState>, name: String, pem: String) -> Result<ferra_core::store::SavedKey, String> {
    state.store.import_key(&name, &pem).map_err(map_err)
}

#[tauri::command]
fn delete_key(state: State<AppState>, id: String) -> Result<(), String> {
    state.store.delete_key(&id).map_err(map_err)
}

#[tauri::command]
fn export_keys(state: State<AppState>, password: String, path: String) -> Result<(), String> {
    state.store.verify_password(&password).map_err(map_err)?;
    let blob = state.store.export_keys_bundle().map_err(map_err)?;
    std::fs::write(path, blob).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_keys(state: State<AppState>, path: String) -> Result<u32, String> {
    state.store.import_keys_from_file(std::path::Path::new(&path)).map_err(map_err)
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    state.store.settings().map_err(map_err)
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    state.store.save_settings(&settings).map_err(map_err)
}

#[tauri::command]
fn algorithm_catalog() -> serde_json::Value {
    algs::catalog()
}

#[tauri::command]
async fn connect_saved(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
    accept_unknown: bool,
) -> Result<serde_json::Value, String> {
    let settings = state.store.settings().map_err(map_err)?;
    let sink: Arc<dyn FrameSink> = Arc::new(TauriSink { app });
    let (live, fingerprint) = state
        .sessions
        .connect_saved(&id, cols, rows, accept_unknown, sink, settings.scrollback as usize)
        .await
        .map_err(map_err)?;
    ferra_core::oplog::write("human", "connect", &format!("saved={id} live={}", live.id));
    Ok(serde_json::json!({ "session_id": live.id, "label": live.label, "fingerprint": fingerprint }))
}

#[tauri::command]
async fn connect_quick(
    app: AppHandle,
    state: State<'_, AppState>,
    req: ConnectQuick,
) -> Result<serde_json::Value, String> {
    let settings = state.store.settings().map_err(map_err)?;
    let sink: Arc<dyn FrameSink> = Arc::new(TauriSink { app });
    let private_key_pem = if let Some(id) = req.key_id.as_deref() {
        Some(state.store.load_private_key(id).map_err(map_err)?)
    } else {
        req.private_key_pem.clone()
    };
    let auth = if req.use_agent {
        AuthMethodKind::Agent
    } else if private_key_pem.is_some() {
        AuthMethodKind::Key
    } else {
        AuthMethodKind::Password
    };
    let opts = ConnectOpts {
        host: req.host,
        port: req.port,
        username: req.username,
        auth,
        secret: SessionSecret {
            password: req.password,
            passphrase: req.passphrase,
            private_key_pem: private_key_pem.clone(),
        },
        private_key_pem,
        algs: req.profile,
        expected_fingerprint: None,
        accept_unknown_host: req.accept_unknown_host,
        keepalive: 30,
        compression: false,
        jump: None,
    };
    let label = format!("{}@{}:{}", opts.username, opts.host, opts.port);
    let (live, fingerprint) = state
        .sessions
        .spawn_connected(
            opts,
            label,
            if req.term.is_empty() { settings.default_term.clone() } else { req.term },
            req.cols.max(2),
            req.rows.max(1),
            req.local_echo,
            settings.scrollback as usize,
            sink,
        )
        .await
        .map_err(map_err)?;
    let _ = fingerprint;
    Ok(serde_json::json!({ "session_id": live.id, "label": live.label, "fingerprint": fingerprint }))
}

#[tauri::command]
fn term_write(state: State<AppState>, session_id: String, data: Vec<u8>) -> Result<(), String> {
    if data.iter().any(|b| *b == 3) {
        state.ai.interrupt(&session_id);
    }
    state.sessions.get(&session_id).map_err(map_err)?.write_bytes(data).map_err(map_err)
}

#[tauri::command]
fn term_write_text(state: State<AppState>, session_id: String, text: String) -> Result<(), String> {
    if text.as_bytes().contains(&3) {
        state.ai.interrupt(&session_id);
    }
    if text.contains('\n') || text.contains('\r') {
        ferra_core::oplog::write("human", "command", &format!("session={session_id} cmd={}", text.replace(['\r', '\n'], " ").trim()));
    }
    state.sessions.get(&session_id).map_err(map_err)?.write_bytes(text.into_bytes()).map_err(map_err)
}

#[tauri::command]
fn term_resize(state: State<AppState>, session_id: String, cols: u16, rows: u16) -> Result<ferra_core::term::TermFrame, String> {
    state.sessions.get(&session_id).map_err(map_err)?.resize(cols, rows).map_err(map_err)
}

#[tauri::command]
fn term_scroll(state: State<AppState>, session_id: String, delta: i32) -> Result<TermFrame, String> {
    Ok(state.sessions.get(&session_id).map_err(map_err)?.scroll(delta))
}

#[tauri::command]
fn term_scroll_to(state: State<AppState>, session_id: String, offset: u32) -> Result<TermFrame, String> {
    Ok(state.sessions.get(&session_id).map_err(map_err)?.scroll_to(offset))
}

/// Copy text between two grid points (history-relative lines).
#[tauri::command]
fn term_range_text(
    state: State<AppState>,
    session_id: String,
    start_line: i32,
    start_col: u32,
    end_line: i32,
    end_col: u32,
) -> Result<String, String> {
    Ok(state
        .sessions
        .get(&session_id)
        .map_err(map_err)?
        .range_text(start_line, start_col, end_line, end_col))
}

#[tauri::command]
fn term_frame(state: State<AppState>, session_id: String) -> Result<TermFrame, String> {
    Ok(state.sessions.get(&session_id).map_err(map_err)?.frame())
}

#[tauri::command]
async fn term_cwd(state: State<'_, AppState>, session_id: String) -> Result<String, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    live.remote_cwd().await.map_err(map_err)
}

#[tauri::command]
async fn host_monitor(state: State<'_, AppState>, session_id: String) -> Result<ferra_core::monitor::HostMetrics, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    live.host_metrics().await.map_err(map_err)
}

#[tauri::command]
async fn host_login_history(state: State<'_, AppState>, session_id: String) -> Result<Vec<String>, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    live.host_login_history().await.map_err(map_err)
}

#[tauri::command]
async fn host_auth_log(state: State<'_, AppState>, session_id: String) -> Result<Vec<String>, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    live.host_auth_log().await.map_err(map_err)
}

#[tauri::command]
fn disconnect(state: State<AppState>, session_id: String) -> Result<(), String> {
    state.ai.interrupt(&session_id);
    if let Ok(live) = state.sessions.get(&session_id) {
        let _ = live.write_bytes(vec![0x03]);
        live.close();
    }
    state.ai.forget(&session_id);
    state.sessions.drop_live(&session_id);
    Ok(())
}

#[tauri::command]
async fn sftp_list(state: State<'_, AppState>, session_id: String, path: String) -> Result<Vec<sftp::RemoteEntry>, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::list_dir(&sftp, &path).await.map_err(map_err)
}

#[derive(serde::Serialize)]
struct PathStat {
    exists: bool,
    is_dir: bool,
    path: String,
    name: String,
    size: u64,
    mtime: u64,
}

#[tauri::command]
async fn sftp_stat(state: State<'_, AppState>, session_id: String, path: String) -> Result<PathStat, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    let (exists, is_dir, size, mtime) = sftp::path_stat(&sftp, &path).await;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    Ok(PathStat { exists, is_dir, path, name, size, mtime })
}

#[tauri::command]
fn local_stat(path: String) -> Result<PathStat, String> {
    let p = PathBuf::from(&path);
    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
    match std::fs::metadata(&p) {
        Ok(m) => Ok(PathStat {
            exists: true,
            is_dir: m.is_dir(),
            path,
            name,
            size: m.len(),
            mtime: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }),
        Err(_) => Ok(PathStat { exists: false, is_dir: false, path, name, size: 0, mtime: 0 }),
    }
}

#[tauri::command]
async fn sftp_mkdir(state: State<'_, AppState>, session_id: String, path: String) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::mkdir_p(&sftp, &path).await.map_err(map_err)
}

#[tauri::command]
async fn sftp_create_file(state: State<'_, AppState>, session_id: String, path: String) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::create_file(&sftp, &path).await.map_err(map_err)
}

#[tauri::command]
async fn sftp_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    recursive: bool,
) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    let job = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<sftp::TransferProgress>();
    let app2 = app.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        while let Some(mut p) = rx.recv().await {
            p.session_id = sid.clone();
            let _ = app2.emit("sftp-progress", p);
        }
    });
    sftp::remove_entry_progress(&sftp, &path, recursive, &job, Some(tx))
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn sftp_rename(state: State<'_, AppState>, session_id: String, from: String, to: String) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::rename(&sftp, &from, &to).await.map_err(map_err)
}

#[tauri::command]
async fn sftp_chmod(state: State<'_, AppState>, session_id: String, path: String, mode: u32) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::chmod(&sftp, &path, mode).await.map_err(map_err)
}

#[tauri::command]
async fn sftp_symlink(state: State<'_, AppState>, session_id: String, target: String, link: String) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::symlink(&sftp, &target, &link).await.map_err(map_err)
}

#[tauri::command]
async fn sftp_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    local: String,
    remote: String,
    upload: bool,
    resume: bool,
    overwrite: bool,
) -> Result<u64, String> {
    ferra_core::oplog::write(
        "human",
        if upload { "sftp-upload" } else { "sftp-download" },
        &format!("session={session_id} local={local} remote={remote}"),
    );
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    let settings = state.store.settings().map_err(map_err)?;
    let opts = TransferOptions {
        resume: resume && !overwrite,
        preserve_perms: settings.preserve_perms,
        follow_symlinks: settings.follow_symlinks,
        chunk_size: (settings.chunk_kib as usize) * 1024,
        parallel: settings.parallel_transfers as usize,
        overwrite,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ferra_core::sftp::TransferProgress>();
    let app2 = app.clone();
    let hub = state.ai.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        while let Some(mut p) = rx.recv().await {
            p.session_id = sid.clone();
            hub.note_transfer(&p);
            let _ = app2.emit("sftp-progress", p);
        }
    });
    ferra_core::session::transfer_with_progress(sftp, PathBuf::from(local), remote, upload, opts, tx)
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn sftp_sync_plan(
    state: State<'_, AppState>,
    session_id: String,
    local: String,
    remote: String,
    bidirectional: bool,
) -> Result<sftp::SyncPlan, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    sftp::plan_sync(&sftp, PathBuf::from(local).as_path(), &remote, bidirectional)
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn sftp_sync_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    local: String,
    remote: String,
    plan: sftp::SyncPlan,
) -> Result<u64, String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let sftp = live.ensure_sftp().await.map_err(map_err)?;
    let settings = state.store.settings().map_err(map_err)?;
    let opts = TransferOptions {
        resume: true,
        preserve_perms: settings.preserve_perms,
        follow_symlinks: settings.follow_symlinks,
        chunk_size: (settings.chunk_kib as usize) * 1024,
        parallel: settings.parallel_transfers as usize,
        overwrite: true,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ferra_core::sftp::TransferProgress>();
    let app2 = app.clone();
    let hub = state.ai.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        while let Some(mut p) = rx.recv().await {
            p.session_id = sid.clone();
            hub.note_transfer(&p);
            let _ = app2.emit("sftp-progress", p);
        }
    });
    sftp::apply_sync(&sftp, PathBuf::from(local).as_path(), &remote, &plan, &opts, "sync", Some(tx))
        .await
        .map_err(map_err)
}

#[tauri::command]
fn list_local(path: String) -> Result<Vec<sftp::RemoteEntry>, String> {
    let path = if path.is_empty() || path == "/" || path == "." {
        dirs_next()
    } else {
        PathBuf::from(path)
    };
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let meta = e.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let is_symlink = meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        out.push(sftp::RemoteEntry {
            name: e.file_name().to_string_lossy().into(),
            path: e.path().to_string_lossy().into(),
            is_dir,
            is_symlink,
            size,
            mode: 0,
            mtime: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            longname: String::new(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
fn mkdir_local(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_local_file(path: String) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn remove_local(app: AppHandle, session_id: String, path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    let list = sftp::collect_local_delete(p).map_err(|e| e.to_string())?;
    let job = uuid::Uuid::new_v4().to_string();
    let total = list.len().max(1) as u64;
    let emit = |item: &str, n: u64, finished: bool, error: Option<String>| {
        let _ = app.emit(
            "sftp-progress",
            sftp::TransferProgress {
                job_id: job.clone(),
                path: item.into(),
                transferred: n,
                total,
                direction: sftp::TransferDirection::Delete,
                finished,
                error,
                session_id: session_id.clone(),
            },
        );
    };
    emit(&path, 0, false, None);
    for (i, (item, is_dir)) in list.iter().enumerate() {
        let done = i as u64 + 1;
        let shown = item.to_string_lossy();
        let r = if *is_dir {
            std::fs::remove_dir(item)
        } else {
            std::fs::remove_file(item)
        };
        if let Err(e) = r {
            emit(&shown, done.saturating_sub(1), true, Some(e.to_string()));
            return Err(e.to_string());
        }
        emit(&shown, done, done >= total, None);
    }
    Ok(())
}

#[tauri::command]
fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn generate_key(state: State<AppState>, name: String) -> Result<ferra_core::store::SavedKey, String> {
    state.store.generate_ed25519(&name).map_err(map_err)
}

#[tauri::command]
fn export_vault(state: State<AppState>, path: String) -> Result<(), String> {
    let blob = state.store.export_vault_snapshot().map_err(map_err)?;
    std::fs::write(path, blob).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_vault(state: State<AppState>, path: String) -> Result<(), String> {
    let blob = std::fs::read(path).map_err(|e| e.to_string())?;
    state.store.import_vault_snapshot(&blob).map_err(map_err)
}

#[tauri::command]
async fn start_local_forward(
    state: State<'_, AppState>,
    session_id: String,
    bind: String,
    dest_host: String,
    dest_port: u16,
) -> Result<(), String> {
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let addr: std::net::SocketAddr = bind.parse().map_err(|e: std::net::AddrParseError| e.to_string())?;
    ferra_core::ssh::start_local_forward(live.handle(), addr, dest_host, dest_port)
        .await
        .map_err(map_err)
}

fn dirs_next() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn bind_install_seal() -> Result<(), String> {
    device_seal::bind_install_seal()
}

pub fn unbind_install_seal() -> Result<(), String> {
    device_seal::unbind_install_seal()
}

#[tauri::command]
fn device_seal_ok() -> bool {
    device_seal::verify_install_seal()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("ferrassh=info,ferra_core=info")
        .init();

    let store = Arc::new(Store::open_default().expect("open vault database"));
    let sessions = Arc::new(SessionManager::new(store.clone()));
    let ai = Arc::new(ai::AiHub::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState { store, sessions, ai })
        .setup(|app| {
            use tauri::Manager;
            if let (Some(window), Some(icon)) = (app.get_webview_window("main"), app.default_window_icon()) {
                window.set_icon(icon.clone())?;
            }
            let days = app
                .state::<AppState>()
                .store
                .settings()
                .map(|s| s.log_retain_days.max(1))
                .unwrap_or(5);
            ferra_core::oplog::prune_async(days);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_status,
            vault_init,
            vault_unlock,
            vault_lock,
            vault_change_password,
            list_folders,
            upsert_folder,
            delete_folder,
            list_sessions,
            upsert_session,
            delete_session,
            list_projects,
            upsert_project,
            delete_project,
            list_project_nodes,
            add_project_node,
            remove_project_node,
            oplog_write,
            oplog_dir,
            check_app_update,
            install_app_update,
            list_keys,
            import_key,
            generate_key,
            delete_key,
            export_keys,
            import_keys,
            get_settings,
            save_settings,
            algorithm_catalog,
            connect_saved,
            connect_quick,
            term_write,
            term_write_text,
            term_resize,
            term_scroll,
            term_scroll_to,
            term_range_text,
            term_frame,
            term_cwd,
            host_monitor,
            host_login_history,
            host_auth_log,
            disconnect,
            sftp_list,
            sftp_stat,
            local_stat,
            sftp_mkdir,
            sftp_create_file,
            sftp_remove,
            sftp_rename,
            sftp_chmod,
            sftp_symlink,
            sftp_transfer,
            sftp_sync_plan,
            sftp_sync_apply,
            list_local,
            mkdir_local,
            create_local_file,
            remove_local,
            clipboard_write,
            clipboard_read,
            export_vault,
            import_vault,
            start_local_forward,
            device_seal_ok,
            ai::ai_catalog,
            ai::get_ai_settings,
            ai::save_ai_settings,
            ai::ai_kb_sources,
            ai::ai_kb_list,
            ai::ai_kb_save,
            ai::ai_kb_import,
            ai::ai_kb_delete_source,
            ai::ai_kb_clear,
            ai::ai_kb_save_template,
            ai::ai_prepare,
            ai::ai_cancel,
            ai::ai_resume,
            ai::ai_ask,
            ai::ai_cluster_ask,
            ai::ai_danger_reply,
        ])
        .run(tauri::generate_context!())
        .expect("FerraSSH failed to start");
}
