use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::sftp::{self, TransferOptions, TransferProgress};
use crate::ssh::{self, ConnectOpts, Established, FerraHandler};
use crate::store::{AuthMethodKind, SessionSecret, Store};
use crate::term::{Emulator, TermFrame};
use crate::{Error, Result};

pub trait FrameSink: Send + Sync + 'static {
    fn on_frame(&self, session_id: &str, frame: TermFrame);
    fn on_clipboard(&self, session_id: &str, text: String);
    fn on_closed(&self, session_id: &str, message: String);
}

#[derive(Clone)]
pub struct LiveSession {
    pub id: String,
    pub label: String,
    tx_in: mpsc::UnboundedSender<PtyCmd>,
    emulator: Arc<Mutex<Emulator>>,
    handle: Arc<tokio::sync::Mutex<russh::client::Handle<FerraHandler>>>,
    sftp: Arc<Mutex<Option<Arc<SftpSession>>>>,
}

enum PtyCmd {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

pub struct SessionManager {
    store: Arc<Store>,
    live: Mutex<HashMap<String, LiveSession>>,
}

impl SessionManager {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store, live: Mutex::new(HashMap::new()) }
    }

    pub fn store(&self) -> Arc<Store> {
        self.store.clone()
    }

    pub async fn connect_saved(
        &self,
        saved_id: &str,
        cols: u16,
        rows: u16,
        accept_unknown_host: bool,
        sink: Arc<dyn FrameSink>,
        scrollback: usize,
    ) -> Result<(LiveSession, String)> {
        let saved = self
            .store
            .list_sessions()?
            .into_iter()
            .find(|s| s.id == saved_id)
            .ok_or_else(|| Error::msg("saved session not found"))?;
        let secret = self.store.load_secret(&saved.id)?.unwrap_or(SessionSecret {
            password: None,
            passphrase: None,
            private_key_pem: None,
        });
        let private_key_pem = match (&saved.key_id, &saved.auth_method) {
            (Some(id), AuthMethodKind::Key) => Some(self.store.load_private_key(id)?),
            _ => secret.private_key_pem.clone(),
        };
        let known = self.store.get_known_host(&saved.host, saved.port)?;
        let jump = match saved.jump_host_id.as_ref() {
            Some(id) => Some(Box::new(self.jump_opts(id, accept_unknown_host)?)),
            None => None,
        };
        let opts = ConnectOpts {
            host: saved.host.clone(),
            port: saved.port,
            username: saved.username.clone(),
            auth: saved.auth_method.clone(),
            secret,
            private_key_pem,
            algs: saved.algs.clone(),
            expected_fingerprint: known.map(|k| k.fingerprint),
            accept_unknown_host,
            keepalive: saved.keepalive,
            compression: saved.compression,
            jump,
        };
        self.spawn_connected(opts, saved.name, saved.term, cols, rows, saved.local_echo, scrollback, sink)
            .await
    }

    fn jump_opts(&self, id: &str, accept_unknown_host: bool) -> Result<ConnectOpts> {
        let saved = self
            .store
            .list_sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::msg("jump host not found"))?;
        let secret = self.store.load_secret(&saved.id)?.unwrap_or(SessionSecret {
            password: None,
            passphrase: None,
            private_key_pem: None,
        });
        let known = self.store.get_known_host(&saved.host, saved.port)?;
        let private_key_pem = match (&saved.key_id, &saved.auth_method) {
            (Some(id), AuthMethodKind::Key) => Some(self.store.load_private_key(id)?),
            _ => secret.private_key_pem.clone(),
        };
        Ok(ConnectOpts {
            host: saved.host,
            port: saved.port,
            username: saved.username,
            auth: saved.auth_method,
            private_key_pem,
            secret,
            algs: saved.algs,
            expected_fingerprint: known.map(|k| k.fingerprint),
            accept_unknown_host,
            keepalive: saved.keepalive,
            compression: saved.compression,
            jump: None,
        })
    }

    pub async fn spawn_connected(
        &self,
        opts: ConnectOpts,
        label: String,
        term: String,
        cols: u16,
        rows: u16,
        local_echo: bool,
        scrollback: usize,
        sink: Arc<dyn FrameSink>,
    ) -> Result<(LiveSession, String)> {
        let host = opts.host.clone();
        let port = opts.port;
        let established = ssh::connect(opts).await?;
        self.store.trust_host(ssh::known_from(&host, port, &established))?;
        let fingerprint = established.fingerprint.clone();
        let live = spawn_live(established, label, term, cols, rows, local_echo, scrollback, sink).await?;
        self.live.lock().insert(live.id.clone(), live.clone());
        Ok((live, fingerprint))
    }

    pub fn get(&self, id: &str) -> Result<LiveSession> {
        self.live.lock().get(id).cloned().ok_or_else(|| Error::SessionNotFound(id.into()))
    }

    pub fn list_live(&self) -> Vec<(String, String)> {
        self.live.lock().values().map(|s| (s.id.clone(), s.label.clone())).collect()
    }

    pub fn drop_live(&self, id: &str) {
        if let Some(s) = self.live.lock().remove(id) {
            let _ = s.tx_in.send(PtyCmd::Close);
        }
    }
}

async fn spawn_live(
    established: Established,
    label: String,
    term: String,
    cols: u16,
    rows: u16,
    local_echo: bool,
    scrollback: usize,
    sink: Arc<dyn FrameSink>,
) -> Result<LiveSession> {
    let mut pty = ssh::open_shell(&established.handle, &term, cols as u32, rows as u32).await?;
    let emulator = Arc::new(Mutex::new(Emulator::new(cols, rows, scrollback)));
    {
        let mut em = emulator.lock();
        em.advance(format!("\x1b[38;2;61;205;195mFerraSSH\x1b[0m  {label}  {term} {cols}x{rows}\r\n").as_bytes());
    }
    let (tx_in, mut rx_in) = mpsc::unbounded_channel::<PtyCmd>();
    let id = Uuid::new_v4().to_string();
    let live = LiveSession {
        id: id.clone(),
        label,
        tx_in,
        emulator: emulator.clone(),
        handle: established.handle.clone(),
        sftp: Arc::new(Mutex::new(None)),
    };

    let sink_id = id.clone();
    let sink_task = sink.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                cmd = rx_in.recv() => {
                    match cmd {
                        Some(PtyCmd::Data(bytes)) => {
                            if local_echo {
                                let frame = {
                                    let mut em = emulator.lock();
                                    em.advance(&bytes);
                                    em.snapshot()
                                };
                                sink_task.on_frame(&sink_id, frame);
                            }
                            if pty.data(&bytes[..]).await.is_err() {
                                sink_task.on_closed(&sink_id, "write failed".into());
                                break;
                            }
                        }
                        Some(PtyCmd::Resize { cols, rows }) => {
                            let _ = pty.window_change(cols as u32, rows as u32, 0, 0).await;
                        }
                        Some(PtyCmd::Close) | None => {
                            let _ = pty.eof().await;
                            break;
                        }
                    }
                }
                msg = pty.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { ref data }) | Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                            let (writes, clip, frame) = {
                                let mut em = emulator.lock();
                                em.advance(data);
                                (em.take_pty_writes(), em.take_clipboard(), em.snapshot())
                            };
                            if let Some(clip) = clip {
                                sink_task.on_clipboard(&sink_id, clip);
                            }
                            for w in writes {
                                let _ = pty.data(w.as_bytes()).await;
                            }
                            sink_task.on_frame(&sink_id, frame);
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            sink_task.on_closed(&sink_id, format!("exit {exit_status}"));
                            break;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                            sink_task.on_closed(&sink_id, "disconnected".into());
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        let _ = ssh::disconnect(&established.handle).await;
    });

    sink.on_frame(&id, live.frame());
    Ok(live)
}

impl LiveSession {
    pub fn write_bytes(&self, data: Vec<u8>) -> Result<()> {
        self.tx_in.send(PtyCmd::Data(data)).map_err(|_| Error::msg("pty closed"))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<TermFrame> {
        let frame = {
            let mut em = self.emulator.lock();
            em.resize(cols, rows);
            em.snapshot()
        };
        self.tx_in
            .send(PtyCmd::Resize { cols, rows })
            .map_err(|_| Error::msg("pty closed"))?;
        Ok(frame)
    }

    pub fn frame(&self) -> TermFrame {
        self.emulator.lock().snapshot()
    }

    pub fn all_text(&self) -> String {
        self.emulator.lock().all_text()
    }

    pub fn range_text(&self, a_line: i32, a_col: u32, b_line: i32, b_col: u32) -> String {
        self.emulator
            .lock()
            .text_range(a_line, a_col as usize, b_line, b_col as usize)
    }

    pub fn scroll(&self, delta: i32) -> TermFrame {
        let mut em = self.emulator.lock();
        em.scroll(delta);
        em.snapshot()
    }

    pub fn scroll_to(&self, offset: u32) -> TermFrame {
        let mut em = self.emulator.lock();
        em.scroll_to(offset);
        em.snapshot()
    }

    pub fn handle(&self) -> Arc<tokio::sync::Mutex<russh::client::Handle<FerraHandler>>> {
        self.handle.clone()
    }

    pub async fn ensure_sftp(&self) -> Result<Arc<SftpSession>> {
        if let Some(s) = self.sftp.lock().clone() {
            return Ok(s);
        }
        let sftp = Arc::new(ssh::open_sftp_channel(&self.handle).await?);
        *self.sftp.lock() = Some(sftp.clone());
        Ok(sftp)
    }

    pub fn close(&self) {
        let _ = self.tx_in.send(PtyCmd::Close);
    }

    pub fn cwd(&self) -> String {
        self.emulator.lock().infer_cwd()
    }

    pub async fn exec_capture(&self, command: &str) -> Result<String> {
        ssh::exec_capture(&self.handle, command).await
    }

    async fn remote_home(&self) -> Result<String> {
        let out = self.exec_capture("printf %s \"$HOME\"").await?;
        let home = out.lines().last().unwrap_or(out.trim()).trim().to_string();
        if home.starts_with('/') {
            Ok(home)
        } else {
            Err(Error::msg("HOME not absolute"))
        }
    }

    async fn expand_remote_path(&self, path: String) -> Result<String> {
        let t = path.trim();
        if t == "~" || t == "~/" {
            return self.remote_home().await;
        }
        if let Some(rest) = t.strip_prefix("~/") {
            let home = self.remote_home().await?;
            return Ok(format!("{}/{}", home.trim_end_matches('/'), rest));
        }
        Ok(t.to_string())
    }

    pub async fn remote_cwd(&self) -> Result<String> {
        let tracked = self.cwd();
        if tracked.starts_with('/') || tracked.starts_with('~') {
            return self.expand_remote_path(tracked).await;
        }
        let out = self.exec_capture("pwd -P 2>/dev/null || pwd").await?;
        let path = out.lines().last().unwrap_or(out.trim()).trim().to_string();
        if path.is_empty() {
            Err(Error::msg("pwd returned empty"))
        } else {
            self.expand_remote_path(path).await
        }
    }

    pub async fn host_metrics(&self) -> Result<crate::monitor::HostMetrics> {
        let out = self.exec_capture(crate::monitor::collect_script()).await?;
        crate::monitor::parse_metrics(&out)
    }

    pub async fn host_login_history(&self) -> Result<Vec<String>> {
        let out = self.exec_capture(crate::monitor::login_history_script()).await?;
        Ok(crate::monitor::parse_tagged_lines(&out, "LAST"))
    }

    pub async fn host_auth_log(&self) -> Result<Vec<String>> {
        let out = self.exec_capture(crate::monitor::auth_log_script()).await?;
        Ok(crate::monitor::parse_tagged_lines(&out, "AUTH"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectQuick {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key_pem: Option<String>,
    pub passphrase: Option<String>,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub use_agent: bool,
    pub profile: crate::algs::AlgSpec,
    pub accept_unknown_host: bool,
    pub local_echo: bool,
    pub term: String,
    pub cols: u16,
    pub rows: u16,
}

pub async fn transfer_with_progress(
    sftp: Arc<SftpSession>,
    local: std::path::PathBuf,
    remote: String,
    upload: bool,
    opts: TransferOptions,
    tx: mpsc::UnboundedSender<TransferProgress>,
) -> Result<u64> {
    let job = Uuid::new_v4().to_string();
    if upload {
        if local.is_dir() {
            sftp::upload_tree(&sftp, &local, &remote, &opts, &job, Some(tx)).await
        } else {
            sftp::upload_file(&sftp, &local, &remote, &opts, &job, Some(tx)).await
        }
    } else {
        let meta = sftp.metadata(&remote).await.map_err(Error::from)?;
        if meta.is_dir() {
            sftp::download_tree(&sftp, &remote, &local, &opts, &job, Some(tx)).await
        } else {
            sftp::download_file(&sftp, &remote, &local, &opts, &job, Some(tx)).await
        }
    }
}
