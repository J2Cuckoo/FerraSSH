use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Key, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use ferra_core::monitor::HostMetrics;
use ferra_core::session::{FrameSink, SessionManager};
use ferra_core::sftp::{self, RemoteEntry};
use ferra_core::store::{AppSettings, AuthMethodKind, Folder, SavedSession, SessionSecret, Store};
use ferra_core::term::TermFrame;
use ferra_core::{AlgSpec, Error};
use tokio::runtime::Runtime;

const ACC: Color32 = Color32::from_rgb(0x3d, 0xcd, 0xc3);
const BG: Color32 = Color32::from_rgb(0x0b, 0x0f, 0x14);
const BG2: Color32 = Color32::from_rgb(0x12, 0x18, 0x21);
const BG3: Color32 = Color32::from_rgb(0x1a, 0x23, 0x30);
const LINE: Color32 = Color32::from_rgb(0x24, 0x30, 0x44);
const FG: Color32 = Color32::from_rgb(0xd6, 0xde, 0xeb);
const MUTED: Color32 = Color32::from_rgb(0x8a, 0xa0, 0xb4);
const DANGER: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    Setup,
    Lock,
    App,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Term,
    Sftp,
    Monitor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Main,
    About,
    Settings,
}

struct Tab {
    id: String,
    label: String,
    saved_id: Option<String>,
    side: Side,
    frame: Option<TermFrame>,
    closed: Option<String>,
}

struct HostPrompt {
    mismatch: bool,
    host: String,
    port: u16,
    fingerprint: String,
    expected: Option<String>,
    saved_id: String,
}

enum Net {
    Unlocked,
    UnlockFailed(String),
    Frame { id: String, frame: TermFrame },
    Clipboard { text: String },
    Closed { id: String, message: String },
    Connected { id: String, label: String, saved_id: Option<String> },
    ConnectFailed(String),
    HostKey(HostPrompt),
    Sftp { id: String, local: Vec<RemoteEntry>, remote: Vec<RemoteEntry>, local_path: String, remote_path: String },
    Metrics { id: String, data: HostMetrics },
}

struct GuiSink {
    tx: Sender<Net>,
}

impl FrameSink for GuiSink {
    fn on_frame(&self, session_id: &str, frame: TermFrame) {
        let _ = self.tx.send(Net::Frame { id: session_id.into(), frame });
    }
    fn on_clipboard(&self, _session_id: &str, text: String) {
        let _ = self.tx.send(Net::Clipboard { text });
    }
    fn on_closed(&self, session_id: &str, message: String) {
        let _ = self.tx.send(Net::Closed { id: session_id.into(), message });
    }
}

struct Draft {
    name: String,
    host: String,
    port: String,
    username: String,
    password: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: "22".into(),
            username: "root".into(),
            password: String::new(),
        }
    }
}

pub struct FerraApp {
    rt: Arc<Runtime>,
    store: Arc<Store>,
    sessions: Arc<SessionManager>,
    tx: Sender<Net>,
    rx: Receiver<Net>,
    gate: Gate,
    page: Page,
    pw: String,
    pw2: String,
    unlocking: bool,
    connecting: bool,
    err: String,
    folders: Vec<Folder>,
    saved: Vec<SavedSession>,
    tabs: Vec<Tab>,
    active: Option<String>,
    settings: AppSettings,
    maximized: bool,
    show_new: bool,
    draft: Draft,
    host_prompt: Option<HostPrompt>,
    sftp_local: Vec<RemoteEntry>,
    sftp_remote: Vec<RemoteEntry>,
    sftp_local_path: String,
    sftp_remote_path: String,
    metrics: Option<HostMetrics>,
    last_metrics: Instant,
    busy_until: Instant,
}

impl FerraApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let rt = Arc::new(Runtime::new().expect("tokio"));
        let store = Arc::new(Store::open_default().expect("open vault"));
        let sessions = Arc::new(SessionManager::new(store.clone()));
        let (tx, rx) = mpsc::channel();
        let initialized = store.is_initialized().unwrap_or(false);
        let unlocked = store.is_unlocked();
        let gate = if unlocked {
            Gate::App
        } else if initialized {
            Gate::Lock
        } else {
            Gate::Setup
        };
        let mut app = Self {
            rt,
            store,
            sessions,
            tx,
            rx,
            gate,
            page: Page::Main,
            pw: String::new(),
            pw2: String::new(),
            unlocking: false,
            connecting: false,
            err: String::new(),
            folders: Vec::new(),
            saved: Vec::new(),
            tabs: Vec::new(),
            active: None,
            settings: AppSettings::sane_default(),
            maximized: false,
            show_new: false,
            draft: Draft::default(),
            host_prompt: None,
            sftp_local: Vec::new(),
            sftp_remote: Vec::new(),
            sftp_local_path: String::new(),
            sftp_remote_path: String::new(),
            metrics: None,
            last_metrics: Instant::now() - Duration::from_secs(10),
            busy_until: Instant::now(),
        };
        if gate == Gate::App {
            app.reload_lists();
        }
        app
    }

    fn reload_lists(&mut self) {
        self.folders = self.store.list_folders().unwrap_or_default();
        self.saved = self.store.list_sessions().unwrap_or_default();
        if let Ok(s) = self.store.settings() {
            self.settings = s;
        }
    }

    fn flash(&mut self) {
        self.busy_until = Instant::now() + Duration::from_millis(280);
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Net::Unlocked => {
                    self.unlocking = false;
                    self.pw.clear();
                    self.pw2.clear();
                    self.gate = Gate::App;
                    self.reload_lists();
                    self.flash();
                }
                Net::UnlockFailed(e) => {
                    self.unlocking = false;
                    self.err = e;
                }
                Net::Frame { id, frame } => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                        t.frame = Some(frame);
                    }
                }
                Net::Clipboard { text } => ctx.copy_text(text),
                Net::Closed { id, message } => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                        t.closed = Some(message);
                    }
                }
                Net::Connected { id, label, saved_id } => {
                    self.connecting = false;
                    self.tabs.push(Tab {
                        id: id.clone(),
                        label,
                        saved_id,
                        side: Side::Term,
                        frame: None,
                        closed: None,
                    });
                    self.active = Some(id);
                    self.flash();
                }
                Net::ConnectFailed(e) => {
                    self.connecting = false;
                    self.err = e;
                }
                Net::HostKey(p) => {
                    self.connecting = false;
                    self.host_prompt = Some(p);
                }
                Net::Sftp { id, local, remote, local_path, remote_path } => {
                    if self.active.as_deref() == Some(id.as_str()) {
                        self.sftp_local = local;
                        self.sftp_remote = remote;
                        self.sftp_local_path = local_path;
                        self.sftp_remote_path = remote_path;
                    }
                }
                Net::Metrics { id, data } => {
                    if self.active.as_deref() == Some(id.as_str()) {
                        self.metrics = Some(data);
                    }
                }
            }
        }
    }

    fn submit_unlock(&mut self) {
        if self.unlocking {
            return;
        }
        self.err.clear();
        let setup = self.gate == Gate::Setup;
        if setup {
            if self.pw.len() < 8 {
                self.err = "主密码至少 8 位".into();
                return;
            }
            if self.pw != self.pw2 {
                self.err = "两次密码不一致".into();
                return;
            }
        }
        self.unlocking = true;
        let store = self.store.clone();
        let pw = self.pw.clone();
        let tx = self.tx.clone();
        self.rt.spawn_blocking(move || {
            let r = if setup { store.initialize(&pw) } else { store.unlock(&pw) };
            let _ = tx.send(match r {
                Ok(()) => Net::Unlocked,
                Err(e) => Net::UnlockFailed(e.to_string()),
            });
        });
    }

    fn connect_saved(&mut self, saved_id: String, accept: bool) {
        if self.connecting {
            return;
        }
        self.connecting = true;
        self.err.clear();
        let sessions = self.sessions.clone();
        let tx = self.tx.clone();
        let sink: Arc<dyn FrameSink> = Arc::new(GuiSink { tx: tx.clone() });
        let scrollback = self.settings.scrollback as usize;
        self.rt.spawn(async move {
            match sessions.connect_saved(&saved_id, 120, 32, accept, sink, scrollback).await {
                Ok((live, _)) => {
                    let _ = tx.send(Net::Connected {
                        id: live.id,
                        label: live.label,
                        saved_id: Some(saved_id),
                    });
                }
                Err(Error::UnknownHostKey { host, port, fingerprint }) => {
                    let _ = tx.send(Net::HostKey(HostPrompt {
                        mismatch: false,
                        host,
                        port,
                        fingerprint,
                        expected: None,
                        saved_id,
                    }));
                }
                Err(Error::HostKeyMismatch { host, port, expected, actual }) => {
                    let _ = tx.send(Net::HostKey(HostPrompt {
                        mismatch: true,
                        host,
                        port,
                        fingerprint: actual,
                        expected: Some(expected),
                        saved_id,
                    }));
                }
                Err(e) => {
                    let _ = tx.send(Net::ConnectFailed(e.to_string()));
                }
            }
        });
    }

    fn close_tab(&mut self, id: String) {
        let idx = self.tabs.iter().position(|t| t.id == id);
        if let Ok(live) = self.sessions.get(&id) {
            live.close();
        }
        self.sessions.drop_live(&id);
        let was_active = self.active.as_deref() == Some(id.as_str());
        self.tabs.retain(|t| t.id != id);
        if was_active {
            let next = idx.and_then(|i| {
                if i > 0 {
                    self.tabs.get(i - 1)
                } else {
                    self.tabs.first()
                }
            });
            self.active = next.map(|t| t.id.clone());
            if self.active.is_some() {
                self.flash();
            }
        }
    }

    fn current(&self) -> Option<&Tab> {
        let id = self.active.as_ref()?;
        self.tabs.iter().find(|t| t.id == *id).or(self.tabs.first())
    }

    fn refresh_sftp(&mut self) {
        let Some(tab) = self.current() else { return };
        if tab.side != Side::Sftp {
            return;
        }
        let id = tab.id.clone();
        let sessions = self.sessions.clone();
        let tx = self.tx.clone();
        let remote_path = if self.sftp_remote_path.is_empty() {
            ".".into()
        } else {
            self.sftp_remote_path.clone()
        };
        let local_path = self.sftp_local_path.clone();
        self.rt.spawn(async move {
            let local = list_local(&local_path);
            let remote = match sessions.get(&id) {
                Ok(live) => match live.ensure_sftp().await {
                    Ok(sftp) => sftp::list_dir(&sftp, &remote_path).await.unwrap_or_default(),
                    Err(_) => Vec::new(),
                },
                Err(_) => Vec::new(),
            };
            let _ = tx.send(Net::Sftp {
                id,
                local,
                remote,
                local_path,
                remote_path,
            });
        });
    }

    fn poll_metrics(&mut self) {
        let (id, side) = match self.current() {
            Some(t) => (t.id.clone(), t.side),
            None => return,
        };
        if side != Side::Monitor {
            return;
        }
        if self.last_metrics.elapsed() < Duration::from_millis(2500) {
            return;
        }
        self.last_metrics = Instant::now();
        let sessions = self.sessions.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            if let Ok(live) = sessions.get(&id) {
                if let Ok(data) = live.host_metrics().await {
                    let _ = tx.send(Net::Metrics { id, data });
                }
            }
        });
    }
}

impl eframe::App for FerraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);
        self.poll_metrics();
        ctx.request_repaint_after(Duration::from_millis(33));

        egui::TopBottomPanel::top("titlebar").exact_height(32.0).frame(egui::Frame::NONE.fill(Color32::from_rgb(0x0e, 0x14, 0x1c))).show(ctx, |ui| {
            self.titlebar(ui, ctx);
        });

        if Instant::now() < self.busy_until {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("加载中…").color(MUTED));
                });
            });
            return;
        }

        match self.gate {
            Gate::Setup | Gate::Lock => {
                if self.page == Page::About {
                    self.about(ctx);
                } else {
                    self.gate_ui(ctx);
                }
            }
            Gate::App => match self.page {
                Page::About => self.about(ctx),
                Page::Settings => self.settings_ui(ctx),
                Page::Main => self.main_ui(ctx),
            },
        }

        if let Some(prompt) = self.host_prompt.take() {
            let mut keep = true;
            egui::Window::new("主机密钥")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if prompt.mismatch {
                        ui.label("主机密钥与记录不一致，可能存在中间人攻击。");
                        if let Some(exp) = &prompt.expected {
                            ui.monospace(format!("记录: {exp}"));
                        }
                    } else {
                        ui.label("首次连接，请核对指纹后信任。");
                    }
                    ui.monospace(format!("{}:{}", prompt.host, prompt.port));
                    ui.monospace(&prompt.fingerprint);
                    ui.horizontal(|ui| {
                        if ui.button("信任并连接").clicked() {
                            self.connect_saved(prompt.saved_id.clone(), true);
                            keep = false;
                        }
                        if ui.button("取消").clicked() {
                            keep = false;
                        }
                    });
                });
            if keep {
                self.host_prompt = Some(prompt);
            }
        }
    }
}

impl FerraApp {
    fn titlebar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);
            ui.colored_label(ACC, "Fs");
            ui.strong("FerraSSH");
            ui.add_space(8.0);
            let settings_on = self.gate == Gate::App;
            ui.add_enabled_ui(settings_on, |ui| {
                if ui.button("设置").clicked() {
                    self.flash();
                    self.page = Page::Settings;
                }
            });
            if ui.button("关于").clicked() {
                self.flash();
                self.page = if self.page == Page::About { Page::Main } else { Page::About };
            }
            let drag = ui.allocate_ui(Vec2::new(ui.available_width() - 138.0, 32.0), |ui| {
                ui.allocate_rect(ui.max_rect(), Sense::click_and_drag())
            });
            if drag.inner.dragged() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.inner.double_clicked() {
                self.maximized = !self.maximized;
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
            }
            if ui.button("—").on_hover_text("最小化").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            let max_label = if self.maximized { "❐" } else { "□" };
            if ui.button(max_label).on_hover_text(if self.maximized { "还原" } else { "最大化" }).clicked() {
                self.maximized = !self.maximized;
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
            }
            if ui.add(egui::Button::new("×").fill(Color32::TRANSPARENT)).on_hover_text("关闭").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn gate_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                egui::Frame::NONE.fill(BG2).stroke(Stroke::new(1.0_f32, LINE)).inner_margin(28.0).corner_radius(14.0).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.heading("FerraSSH");
                    ui.label(egui::RichText::new("主密码保护的 SSH / SFTP 工作站").color(MUTED));
                    ui.add_space(12.0);
                    ui.label("主密码");
                    let pw = egui::TextEdit::singleline(&mut self.pw).password(true).desired_width(360.0);
                    let resp = ui.add_enabled(!self.unlocking, pw);
                    if self.gate == Gate::Setup {
                        ui.label("确认主密码");
                        ui.add_enabled(!self.unlocking, egui::TextEdit::singleline(&mut self.pw2).password(true).desired_width(360.0));
                    }
                    if !self.err.is_empty() {
                        ui.colored_label(DANGER, &self.err);
                    }
                    let label = if self.unlocking {
                        if self.gate == Gate::Setup { "正在创建…" } else { "正在解锁…" }
                    } else if self.gate == Gate::Setup {
                        "创建保险库"
                    } else {
                        "解锁"
                    };
                    let go = ui.add_enabled(!self.unlocking, egui::Button::new(label).min_size(Vec2::new(360.0, 36.0)));
                    if (go.clicked() || (resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))) && !self.unlocking {
                        self.submit_unlock();
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("连接从容，如在本地。").color(MUTED));
                });
            });
        });
    }

    fn about(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG).inner_margin(32.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("FerraSSH");
                ui.label("原生 SSH / SFTP 工作站");
                if ui.button("返回").clicked() {
                    self.flash();
                    self.page = Page::Main;
                }
            });
            ui.label(format!("v{}  纯 Rust 静态二进制，无 WebView", env!("CARGO_PKG_VERSION")));
            ui.add_space(16.0);
            ui.label("版权所有 © 2026 贵州力贤网络科技有限公司");
            ui.label("许可证 MIT");
        });
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG).inner_margin(24.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("设置");
                if ui.button("返回").clicked() {
                    self.flash();
                    self.page = Page::Main;
                }
            });
            ui.add(egui::Slider::new(&mut self.settings.font_size, 10.0..=22.0).text("字体大小"));
            ui.add(egui::Slider::new(&mut self.settings.scrollback, 1000..=50000).text("回滚行数"));
            if ui.button("保存").clicked() {
                let _ = self.store.save_settings(&self.settings);
                self.flash();
                self.page = Page::Main;
            }
        });
    }

    fn main_ui(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sess")
            .exact_width(280.0)
            .frame(egui::Frame::NONE.fill(BG2).stroke(Stroke::new(1.0_f32, LINE)).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("新建会话").fill(ACC)).clicked() {
                        self.show_new = true;
                    }
                    if ui.button("新建分组").clicked() {
                        let f = Folder {
                            id: uuid::Uuid::new_v4().to_string(),
                            parent_id: None,
                            name: "新分组".into(),
                            sort_order: self.folders.len() as i64,
                            kind: "ops".into(),
                        };
                        let _ = self.store.upsert_folder(&f);
                        self.reload_lists();
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("会话").color(MUTED));
                let saved = self.saved.clone();
                let folders = self.folders.clone();
                for f in &folders {
                    ui.strong(&f.name);
                    for s in saved.iter().filter(|s| s.folder_id.as_deref() == Some(f.id.as_str())) {
                        self.session_row(ui, s);
                    }
                }
                for s in saved.iter().filter(|s| s.folder_id.is_none()) {
                    self.session_row(ui, s);
                }
            });

        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(ctx, |ui| {
            self.tabs_row(ui);
            if let Some(err) = (!self.err.is_empty()).then_some(self.err.clone()) {
                ui.colored_label(DANGER, err);
            }
            if self.tabs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("双击左侧会话连接，或新建一个主机。").color(MUTED));
                });
                return;
            }
            let (id, side, frame, closed) = {
                let tab = self.current();
                match tab {
                    Some(t) => (t.id.clone(), t.side, t.frame.clone(), t.closed.clone()),
                    None => return,
                }
            };
            match side {
                Side::Term => self.term_pane(ui, &id, frame.as_ref()),
                Side::Sftp => self.sftp_pane(ui),
                Side::Monitor => self.monitor_pane(ui),
            }
            if let Some(msg) = closed {
                ui.colored_label(DANGER, msg);
            }
        });

        if self.show_new {
            let mut open = true;
            egui::Window::new("新建会话").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名称");
                    ui.text_edit_singleline(&mut self.draft.name);
                });
                ui.horizontal(|ui| {
                    ui.label("主机");
                    ui.text_edit_singleline(&mut self.draft.host);
                });
                ui.horizontal(|ui| {
                    ui.label("端口");
                    ui.text_edit_singleline(&mut self.draft.port);
                    ui.label("用户");
                    ui.text_edit_singleline(&mut self.draft.username);
                });
                ui.horizontal(|ui| {
                    ui.label("密码");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.password).password(true));
                });
                ui.horizontal(|ui| {
                    if ui.button("保存并连接").clicked() {
                        if let Some(id) = self.save_draft() {
                            self.show_new = false;
                            self.connect_saved(id, false);
                        }
                    }
                    if ui.button("取消").clicked() {
                        self.show_new = false;
                    }
                });
            });
            if !open {
                self.show_new = false;
            }
        }
    }

    fn save_draft(&mut self) -> Option<String> {
        let host = self.draft.host.trim();
        if host.is_empty() {
            self.err = "请填写主机".into();
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let session = SavedSession {
            id: id.clone(),
            folder_id: None,
            name: if self.draft.name.is_empty() { host.into() } else { self.draft.name.clone() },
            host: host.into(),
            port: self.draft.port.parse().unwrap_or(22),
            username: self.draft.username.clone(),
            auth_method: AuthMethodKind::Password,
            key_id: None,
            jump_host_id: None,
            algs: AlgSpec::modern(),
            local_echo: false,
            keepalive: 30,
            compression: false,
            term: self.settings.default_term.clone(),
            notes: String::new(),
            sort_order: self.saved.len() as i64,
            updated_at: 0,
            has_secret: true,
            in_ops: true,
            sftp_local_path: String::new(),
        };
        let secret = SessionSecret {
            password: Some(self.draft.password.clone()),
            passphrase: None,
            private_key_pem: None,
        };
        if let Err(e) = self.store.upsert_session(&session, Some(&secret)) {
            self.err = e.to_string();
            return None;
        }
        self.reload_lists();
        Some(id)
    }

    fn session_row(&mut self, ui: &mut Ui, s: &SavedSession) {
        let live = self.tabs.iter().any(|t| t.saved_id.as_deref() == Some(s.id.as_str()));
        let caption = format!("{}@{}", s.username, s.host);
        let resp = ui.add(egui::Button::new(format!("{}\n{caption}", s.name)).sense(Sense::click()));
        if live {
            ui.colored_label(ACC, "●");
        }
        if resp.double_clicked() {
            self.connect_saved(s.id.clone(), false);
        }
        resp.context_menu(|ui| {
            if ui.button("删除").clicked() {
                let _ = self.store.delete_session(&s.id);
                self.reload_lists();
                ui.close_menu();
            }
        });
    }

    fn tabs_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let tabs: Vec<(String, String, bool)> = self
                .tabs
                .iter()
                .map(|t| (t.id.clone(), t.label.clone(), self.active.as_deref() == Some(t.id.as_str())))
                .collect();
            let mut close = None;
            let mut activate = None;
            for (id, label, on) in tabs {
                ui.horizontal(|ui| {
                    let btn = ui.selectable_label(on, &label);
                    if btn.clicked() && !on {
                        activate = Some(id.clone());
                    }
                    if ui.small_button("×").clicked() {
                        close = Some(id);
                    }
                });
            }
            if let Some(id) = activate {
                self.flash();
                self.active = Some(id);
            }
            if let Some(id) = close {
                self.close_tab(id);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.current().is_some() {
                    for (side, name) in [(Side::Monitor, "监控"), (Side::Sftp, "SFTP"), (Side::Term, "终端")] {
                        let on = self.current().map(|t| t.side == side).unwrap_or(false);
                        if ui.selectable_label(on, name).clicked() && !on {
                            self.flash();
                            if let Some(t) = self.tabs.iter_mut().find(|t| Some(t.id.as_str()) == self.active.as_deref()) {
                                t.side = side;
                            }
                            if side == Side::Sftp {
                                self.refresh_sftp();
                            }
                        }
                    }
                }
            });
        });
        ui.separator();
    }

    fn term_pane(&mut self, ui: &mut Ui, id: &str, frame: Option<&TermFrame>) {
        let font = FontId::monospace(self.settings.font_size);
        let row_h = self.settings.font_size + 4.0;
        let col_w = self.settings.font_size * 0.62;
        let avail = ui.available_size();
        let cols = ((avail.x / col_w).floor() as u16).max(2);
        let rows = ((avail.y / row_h).floor() as u16).max(1);
        if let Ok(live) = self.sessions.get(id) {
            let _ = live.resize(cols, rows);
        }
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        if resp.clicked() {
            resp.request_focus();
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(11, 15, 20));
        if let Some(frame) = frame {
            for line in &frame.lines {
                for (x, cell) in line.cells.iter().enumerate() {
                    let pos = Pos2::new(rect.left() + x as f32 * col_w, rect.top() + line.y as f32 * row_h);
                    let bg = rgb(cell.bg);
                    let fg = rgb(cell.fg);
                    painter.rect_filled(Rect::from_min_size(pos, Vec2::new(col_w, row_h)), 0.0, bg);
                    if cell.ch != ' ' && cell.ch != '\0' {
                        painter.text(pos, egui::Align2::LEFT_TOP, cell.ch, font.clone(), fg);
                    }
                }
            }
            if frame.cursor_visible {
                let cpos = Pos2::new(
                    rect.left() + frame.cursor_x as f32 * col_w,
                    rect.top() + frame.cursor_y as f32 * row_h,
                );
                painter.rect_filled(Rect::from_min_size(cpos, Vec2::new(col_w.max(2.0), row_h)), 0.0, ACC);
            }
        }
        if resp.has_focus() || resp.clicked() {
            ui.input(|i| {
                for ev in &i.events {
                    if let Some(bytes) = event_to_bytes(ev, frame) {
                        if let Ok(live) = self.sessions.get(id) {
                            let _ = live.write_bytes(bytes);
                        }
                    }
                }
                let scroll = i.raw_scroll_delta.y;
                if scroll.abs() > 0.1 {
                    if let Ok(live) = self.sessions.get(id) {
                        let f = live.scroll(if scroll > 0.0 { 3 } else { -3 });
                        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                            t.frame = Some(f);
                        }
                    }
                }
            });
        }
    }

    fn sftp_pane(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("刷新").clicked() {
                self.refresh_sftp();
            }
            ui.label(format!("本地 {}", self.sftp_local_path));
            ui.label(format!("远端 {}", self.sftp_remote_path));
        });
        ui.columns(2, |cols| {
            cols[0].label("本地");
            egui::ScrollArea::vertical().id_salt("local").show(&mut cols[0], |ui| {
                for e in &self.sftp_local {
                    ui.label(format!("{} {}", if e.is_dir { "📁" } else { "📄" }, e.name));
                }
            });
            cols[1].label("远端");
            egui::ScrollArea::vertical().id_salt("remote").show(&mut cols[1], |ui| {
                for e in &self.sftp_remote {
                    ui.label(format!("{} {}", if e.is_dir { "📁" } else { "📄" }, e.name));
                }
            });
        });
    }

    fn monitor_pane(&mut self, ui: &mut Ui) {
        match &self.metrics {
            None => {
                ui.spinner();
                ui.label("正在采集主机指标…");
            }
            Some(m) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(format!("主机  {}", m.hostname));
                    ui.label(format!("系统  {}", if m.os_name.is_empty() { &m.uname } else { &m.os_name }));
                    ui.label(format!("内核  {}", m.kernel));
                    ui.label(format!("CPU   {}  {:.0}%", m.cpu_model, m.cpu_percent));
                    ui.label(format!("负载  {:.2} / {:.2} / {:.2}", m.load1, m.load5, m.load15));
                    ui.label(format!("内存  {} / {} KB", m.mem_used_kb, m.mem_total_kb));
                });
            }
        }
    }
}

fn rgb(v: u32) -> Color32 {
    Color32::from_rgb(((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
}

fn event_to_bytes(ev: &egui::Event, frame: Option<&TermFrame>) -> Option<Vec<u8>> {
    let app = frame.map(|f| f.app_cursor).unwrap_or(false);
    match ev {
        egui::Event::Text(t) => Some(t.as_bytes().to_vec()),
        egui::Event::Ime(egui::ImeEvent::Commit(t)) => Some(t.as_bytes().to_vec()),
        egui::Event::Paste(t) => {
            if frame.map(|f| f.bracketed_paste).unwrap_or(false) {
                let mut o = b"\x1b[200~".to_vec();
                o.extend_from_slice(t.as_bytes());
                o.extend_from_slice(b"\x1b[201~");
                Some(o)
            } else {
                Some(t.as_bytes().to_vec())
            }
        }
        egui::Event::Key { key, pressed: true, modifiers, .. } => {
            if modifiers.ctrl {
                return ctrl_key(*key);
            }
            Some(match key {
                Key::Enter => b"\r".to_vec(),
                Key::Backspace => b"\x7f".to_vec(),
                Key::Tab => b"\t".to_vec(),
                Key::Escape => b"\x1b".to_vec(),
                Key::ArrowUp => if app { b"\x1bOA".to_vec() } else { b"\x1b[A".to_vec() },
                Key::ArrowDown => if app { b"\x1bOB".to_vec() } else { b"\x1b[B".to_vec() },
                Key::ArrowRight => if app { b"\x1bOC".to_vec() } else { b"\x1b[C".to_vec() },
                Key::ArrowLeft => if app { b"\x1bOD".to_vec() } else { b"\x1b[D".to_vec() },
                Key::Home => b"\x1b[H".to_vec(),
                Key::End => b"\x1b[F".to_vec(),
                Key::Delete => b"\x1b[3~".to_vec(),
                Key::PageUp => b"\x1b[5~".to_vec(),
                Key::PageDown => b"\x1b[6~".to_vec(),
                _ => return None,
            })
        }
        _ => None,
    }
}

fn ctrl_key(key: Key) -> Option<Vec<u8>> {
    let c = match key {
        Key::C => 3,
        Key::D => 4,
        Key::Z => 26,
        Key::L => 12,
        Key::U => 21,
        Key::W => 23,
        Key::A => 1,
        Key::E => 5,
        Key::K => 11,
        Key::R => 18,
        _ => return None,
    };
    Some(vec![c])
}

fn list_local(path: &str) -> Vec<RemoteEntry> {
    let path = if path.is_empty() || path == "/" || path == "." {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    };
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&path) else {
        return out;
    };
    for e in rd.flatten() {
        let meta = e.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        out.push(RemoteEntry {
            name: e.file_name().to_string_lossy().into(),
            path: e.path().to_string_lossy().into(),
            is_dir,
            is_symlink: meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            mode: 0,
            mtime: 0,
            longname: String::new(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn apply_theme(ctx: &egui::Context) {
    install_cjk_fonts(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG2;
    style.visuals.extreme_bg_color = BG;
    style.visuals.widgets.inactive.bg_fill = BG3;
    style.visuals.widgets.hovered.bg_fill = BG3;
    style.visuals.widgets.active.bg_fill = BG3;
    style.visuals.override_text_color = Some(FG);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, FG);
    style.visuals.selection.bg_fill = ACC.linear_multiply(0.35);
    ctx.set_style(style);
}

fn install_cjk_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut names: Vec<String> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            load_fonts_from_dir(&mut fonts, &mut names, &dir.join("fonts"));
        }
    }

    #[cfg(windows)]
    {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let font_dir = PathBuf::from(windir).join("Fonts");
        for (name, file) in [
            ("yahei", "msyh.ttc"),
            ("yahei-bold", "msyhbd.ttc"),
            ("simhei", "simhei.ttf"),
            ("simsun", "simsun.ttc"),
            ("deng", "Deng.ttf"),
        ] {
            if fonts.font_data.contains_key(name) {
                continue;
            }
            let path = font_dir.join(file);
            if let Ok(bytes) = std::fs::read(&path) {
                if bytes.len() > 1024 {
                    fonts.font_data.insert(name.to_owned(), FontData::from_owned(bytes).into());
                    names.push(name.to_owned());
                }
            }
        }
    }

    if names.is_empty() {
        return;
    }
    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        for name in names.iter().rev() {
            proportional.insert(0, name.clone());
        }
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        for name in &names {
            if !mono.contains(name) {
                mono.push(name.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn load_fonts_from_dir(fonts: &mut FontDefinitions, names: &mut Vec<String>, dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() > 1024 {
                let key = format!("app-{stem}");
                fonts.font_data.insert(key.clone(), FontData::from_owned(bytes).into());
                names.push(key);
            }
        }
    }
}
