use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferra_core::ai::{self, AiSettings, KbDoc, KbSourceSummary, LlmStep, DONE_MARK};
use ferra_core::session::SessionManager;
use ferra_core::sftp::{self, TransferDirection, TransferProgress};
use ferra_core::store::Store;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::{map_err, AppState};

#[derive(Clone)]
struct DangerReply {
    approved: bool,
    command: String,
}

#[derive(Default)]
struct SessionAi {
    cancel: Arc<AtomicBool>,
    busy: bool,
    facts: String,
    history: Vec<(String, String)>,
    inbox: Vec<String>,
    resume: bool,
    inflight_uploads: HashSet<String>,
    completed_uploads: u64,
    danger_reply: Option<DangerReply>,
    cluster_nodes: HashMap<String, String>,
}

pub struct AiHub {
    sessions: Mutex<HashMap<String, SessionAi>>,
    pub client: reqwest::Client,
}

impl AiHub {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionAi>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn try_begin(&self, session_id: &str) -> Option<Arc<AtomicBool>> {
        let mut map = self.map();
        let slot = map.entry(session_id.to_string()).or_default();
        if slot.busy {
            return None;
        }
        slot.busy = true;
        slot.cancel = Arc::new(AtomicBool::new(false));
        slot.resume = false;
        Some(slot.cancel.clone())
    }

    pub fn end_busy(&self, session_id: &str) {
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.busy = false;
        }
    }

    pub fn interrupt(&self, session_id: &str) {
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.cancel.store(true, Ordering::SeqCst);
            slot.resume = true;
        }
    }

    pub fn forget(&self, session_id: &str) {
        if let Some(slot) = self.map().remove(session_id) {
            slot.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn set_facts(&self, session_id: &str, facts: String) {
        self.map().entry(session_id.to_string()).or_default().facts = facts;
    }

    pub fn facts(&self, session_id: &str) -> String {
        self.map().get(session_id).map(|s| s.facts.clone()).unwrap_or_default()
    }

    pub fn history(&self, session_id: &str) -> Vec<(String, String)> {
        self.map().get(session_id).map(|s| s.history.clone()).unwrap_or_default()
    }

    pub fn save_history(&self, session_id: &str, mut history: Vec<(String, String)>) {
        const MAX: usize = 80;
        if history.len() > MAX {
            let first = history[0].clone();
            let mut tail: Vec<_> = history.drain(history.len() - (MAX - 1)..).collect();
            history.clear();
            if tail.first() != Some(&first) {
                history.push(first);
            }
            history.append(&mut tail);
        }
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.history = history;
        }
    }

    pub fn dropped(&self, session_id: &str) -> bool {
        !self.map().contains_key(session_id)
    }

    pub fn push_inbox(&self, session_id: &str, text: String) {
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.inbox.push(text);
            slot.resume = true;
        }
    }

    pub fn take_inbox(&self, session_id: &str) -> Vec<String> {
        self.map()
            .get_mut(session_id)
            .map(|s| std::mem::take(&mut s.inbox))
            .unwrap_or_default()
    }

    pub fn request_resume(&self, session_id: &str) {
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.resume = true;
        }
    }

    pub fn take_resume(&self, session_id: &str) -> bool {
        self.map().get_mut(session_id).map(|s| {
            let v = s.resume;
            s.resume = false;
            v
        }).unwrap_or(false)
    }

    pub fn note_transfer(&self, p: &TransferProgress) {
        if p.session_id.is_empty() || p.direction != TransferDirection::Upload {
            return;
        }
        let mut map = self.map();
        let slot = map.entry(p.session_id.clone()).or_default();
        if p.finished {
            slot.inflight_uploads.remove(&p.job_id);
            if p.error.is_none() {
                slot.completed_uploads = slot.completed_uploads.saturating_add(1);
            }
        } else {
            slot.inflight_uploads.insert(p.job_id.clone());
        }
    }

    pub fn upload_progress(&self, session_id: &str) -> (u64, usize) {
        self.map()
            .get(session_id)
            .map(|s| (s.completed_uploads, s.inflight_uploads.len()))
            .unwrap_or((0, 0))
    }

    pub fn is_busy(&self, session_id: &str) -> bool {
        self.map().get(session_id).map(|s| s.busy).unwrap_or(false)
    }

    pub fn set_cluster_nodes(&self, session_id: &str, nodes: HashMap<String, String>) {
        self.map().entry(session_id.to_string()).or_default().cluster_nodes = nodes;
    }

    pub fn cluster_nodes(&self, session_id: &str) -> HashMap<String, String> {
        self.map()
            .get(session_id)
            .map(|s| s.cluster_nodes.clone())
            .unwrap_or_default()
    }

    pub fn set_danger_reply(&self, session_id: &str, approved: bool, command: String) {
        if let Some(slot) = self.map().get_mut(session_id) {
            slot.danger_reply = Some(DangerReply { approved, command });
        }
    }

    fn take_danger_reply(&self, session_id: &str) -> Option<DangerReply> {
        self.map().get_mut(session_id).and_then(|s| s.danger_reply.take())
    }
}

#[derive(Clone, Serialize)]
struct AiEvent {
    session_id: String,
    kind: String,
    role: String,
    text: String,
}

fn emit(app: &AppHandle, session_id: &str, kind: &str, role: &str, text: &str) {
    let _ = app.emit(
        "ai-event",
        AiEvent {
            session_id: session_id.into(),
            kind: kind.into(),
            role: role.into(),
            text: text.into(),
        },
    );
}

fn stopped(flag: &AtomicBool) -> bool {
    flag.load(Ordering::SeqCst)
}

fn abandoned(hub: &AiHub, session_id: &str, flag: &AtomicBool) -> bool {
    stopped(flag) || hub.dropped(session_id)
}

async fn until_stopped(flag: &AtomicBool) {
    loop {
        if stopped(flag) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

#[tauri::command]
pub fn ai_catalog() -> serde_json::Value {
    serde_json::to_value(ai::providers()).unwrap_or(serde_json::json!([]))
}

#[tauri::command]
pub fn get_ai_settings(state: State<AppState>) -> Result<AiSettings, String> {
    state.store.ai_settings().map_err(map_err)
}

#[tauri::command]
pub fn save_ai_settings(state: State<AppState>, settings: AiSettings) -> Result<(), String> {
    state.store.save_ai_settings(&settings).map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_sources(state: State<AppState>) -> Result<Vec<KbSourceSummary>, String> {
    state.store.kb_sources().map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_list(state: State<AppState>, source: String) -> Result<Vec<KbDoc>, String> {
    state.store.list_kb_by_source(&source).map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_save(
    state: State<AppState>,
    old_source: String,
    new_source: String,
    docs: Vec<KbDoc>,
) -> Result<(), String> {
    state.store.save_kb_docs(&old_source, &new_source, &docs).map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_import(state: State<AppState>, path: String) -> Result<u32, String> {
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("knowledge.json")
        .to_string();
    state.store.import_kb_json(&name, &raw).map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_save_template(path: String) -> Result<(), String> {
    std::fs::write(path, ai::kb_template_json()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ai_kb_delete_source(state: State<AppState>, source: String) -> Result<(), String> {
    state.store.delete_kb_source(&source).map_err(map_err)
}

#[tauri::command]
pub fn ai_kb_clear(state: State<AppState>) -> Result<(), String> {
    state.store.clear_kb().map_err(map_err)
}

#[tauri::command]
pub async fn ai_prepare(state: State<'_, AppState>, session_id: String) -> Result<String, String> {
    let settings = state.store.ai_settings().map_err(map_err)?;
    if !settings.enabled {
        return Ok(String::new());
    }
    settings.validate_for_enable().map_err(map_err)?;
    let existing = state.ai.facts(&session_id);
    if !existing.trim().is_empty() {
        return Ok(existing);
    }
    let live = state.sessions.get(&session_id).map_err(map_err)?;
    let metrics = live.host_metrics().await.map_err(map_err)?;
    let scope = ai::network_scope(&metrics.nics);
    let nics = metrics
        .nics
        .iter()
        .filter(|n| !n.ipv4.trim().is_empty())
        .map(|n| format!("{} {} {}", n.name, n.state, n.ipv4))
        .collect::<Vec<_>>()
        .join("; ");
    let extra = format!("{} {} {} {}", metrics.os_name, metrics.hostname, scope, nics);
    let kb = state.store.search_kb(&metrics.hostname, &extra, 6).map_err(map_err)?;
    let host_facts = format!(
        "主机名: {}\n系统: {}\n内核: {}\n架构: {}\nCPU: {}\n虚拟化: {}\n网络定位: {}\n网卡: {}\n内存: {} KB\n",
        metrics.hostname,
        metrics.os_name,
        metrics.kernel,
        metrics.arch,
        metrics.cpu_model,
        metrics.virt,
        scope,
        nics,
        metrics.mem_total_kb
    );
    let mut facts = host_facts.clone();
    if !kb.is_empty() {
        // 知识库只进后台上下文，不随 prepare 返回值展示在 AI 面板。
        facts.push_str("\n知识库优先条目:\n");
        for d in &kb {
            let body: String = d.body.chars().take(800).collect();
            facts.push_str(&format!("- [{}] {}\n{}\n", d.source, d.title, body));
        }
    }
    state.ai.set_facts(&session_id, facts);
    Ok(host_facts)
}

#[tauri::command]
pub fn ai_cancel(state: State<AppState>, session_id: String) -> Result<(), String> {
    state.ai.interrupt(&session_id);
    if let Ok(live) = state.sessions.get(&session_id) {
        let _ = live.write_bytes(vec![0x03]);
    }
    Ok(())
}

#[tauri::command]
pub fn ai_resume(state: State<AppState>, session_id: String) -> Result<(), String> {
    state.ai.request_resume(&session_id);
    Ok(())
}

#[tauri::command]
pub fn ai_danger_reply(state: State<AppState>, session_id: String, approved: bool, command: String) -> Result<(), String> {
    state.ai.set_danger_reply(&session_id, approved, command);
    Ok(())
}

#[tauri::command]
pub async fn ai_cluster_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    text: String,
    nodes: Vec<(String, String, String)>,
) -> Result<(), String> {
    let mut map = HashMap::new();
    let mut facts = String::from("这是集群运维任务。优先拉取并分析各节点链路日志，再精准定位问题节点。\n");
    for (saved, live, name) in &nodes {
        map.insert(saved.clone(), live.clone());
        facts.push_str(&format!("节点 {name} id={saved} live={live}\n"));
        if let Ok(sess) = state.sessions.get(live) {
            if let Ok(lines) = sess.host_auth_log().await {
                facts.push_str("近期认证/系统日志摘录：\n");
                for line in lines.iter().rev().take(40) {
                    facts.push_str(line);
                    facts.push('\n');
                }
            }
        }
    }
    state.ai.set_facts(&session_id, facts);
    state.ai.set_cluster_nodes(&session_id, map);
    ai_ask(app, state, session_id, text).await
}

#[tauri::command]
pub async fn ai_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let settings = state.store.ai_settings().map_err(map_err)?;
    if !settings.enabled {
        return Err("请先在 AI 菜单中开启并完成配置".into());
    }
    settings.validate_for_enable().map_err(map_err)?;
    if state.ai.is_busy(&session_id) {
        emit(&app, &session_id, "message", "user", &text);
        state.ai.push_inbox(&session_id, text);
        return Ok(());
    }
    let Some(flag) = state.ai.try_begin(&session_id) else {
        emit(&app, &session_id, "message", "user", &text);
        state.ai.push_inbox(&session_id, text);
        return Ok(());
    };
    let store = state.store.clone();
    let sessions = state.sessions.clone();
    let client = state.ai.client.clone();
    let facts = state.ai.facts(&session_id);
    let hub = state.ai.clone();
    let sid = session_id.clone();
    let result = run_agent(
        app.clone(),
        store,
        sessions,
        client,
        hub.clone(),
        sid.clone(),
        settings,
        facts,
        text,
        flag,
    )
    .await;
    hub.end_busy(&sid);
    if let Err(e) = result {
        emit(&app, &sid, "error", "assistant", &e);
        emit(&app, &sid, "done", "system", "本轮结束");
    }
    Ok(())
}

fn wait_kind(step: &LlmStep) -> Option<String> {
    step.wait
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && *s != "null")
}

async fn run_agent(
    app: AppHandle,
    store: Arc<Store>,
    sessions: Arc<SessionManager>,
    client: reqwest::Client,
    hub: Arc<AiHub>,
    session_id: String,
    settings: AiSettings,
    facts: String,
    user_text: String,
    flag: Arc<AtomicBool>,
) -> Result<(), String> {
    let extra = format!("{facts} {user_text}");
    let kb = store.search_kb(&user_text, &extra, 8).map_err(map_err)?;
    let mut kb_block = String::new();
    if !kb.is_empty() {
        kb_block.push_str("当前问题相关的知识库内容（必须优先遵循）：\n");
        for d in kb {
            let body: String = d.body.chars().take(1200).collect();
            kb_block.push_str(&format!("### {} ({})\n{}\n\n", d.title, d.source, body));
        }
    }
    let mut history = hub.history(&session_id);
    if !history.is_empty() {
        let cont = classify_continuity(&client, &settings, &history, &user_text, &flag).await;
        if cont == Ok(false) {
            history.clear();
        }
    }
    if history.is_empty() {
        history.push((
            "user".into(),
            format!(
                "当前服务器事实：\n{}\n\n{}\n用户需求：\n{}",
                if facts.is_empty() { "（尚未采集到主机信息）" } else { &facts },
                kb_block,
                user_text
            ),
        ));
    } else {
        history.push(("user".into(), user_text.clone()));
    }
    emit(&app, &session_id, "message", "user", &user_text);

    loop {
        if abandoned(&hub, &session_id, &flag) {
            hub.save_history(&session_id, history);
            emit(&app, &session_id, "done", "system", "已打断");
            return Ok(());
        }
        for msg in hub.take_inbox(&session_id) {
            history.push(("user".into(), format!("用户补充：{msg}")));
        }
        emit(&app, &session_id, "waiting", "system", "正在执行计划");
        emit(&app, &session_id, "status", "system", "正在思考…");
        let step = match chat_llm_or_stop(&client, &settings, &history, &flag).await {
            Err(e) if e == "interrupted" => {
                hub.save_history(&session_id, history);
                emit(&app, &session_id, "done", "system", "已打断");
                return Ok(());
            }
            other => other?,
        };
        if abandoned(&hub, &session_id, &flag) {
            emit(&app, &session_id, "done", "system", "已打断");
            return Ok(());
        }
        if !step.say.trim().is_empty() {
            emit(&app, &session_id, "message", "assistant", step.say.trim());
        }
        let cmd = step.command.as_deref().unwrap_or("").trim().to_string();
        let waiting_user = wait_kind(&step).is_some() || (!step.done && cmd.is_empty());
        if waiting_user {
            let kind = wait_kind(&step).unwrap_or_else(|| "user".into());
            let path = step.wait_path.clone().unwrap_or_default();
            let hint = if path.trim().is_empty() {
                let s = step.say.trim();
                if s.is_empty() {
                    "等待你完成本地或终端操作后继续".into()
                } else {
                    s.to_string()
                }
            } else {
                format!("{}（检测：{path}）", step.say.trim())
            };
            emit(&app, &session_id, "wait-user", "system", &hint);
            let note = wait_for_user(&hub, &sessions, &session_id, &kind, path.trim(), &flag).await?;
            if note == "interrupted" || abandoned(&hub, &session_id, &flag) {
                hub.save_history(&session_id, history);
                emit(&app, &session_id, "done", "system", "已打断");
                return Ok(());
            }
            history.push(("assistant".into(), serde_json::to_string(&step).unwrap_or_default()));
            history.push(("user".into(), note));
            hub.save_history(&session_id, history.clone());
            continue;
        }
        if step.done {
            hub.save_history(&session_id, history);
            emit(&app, &session_id, "done", "system", "本轮结束");
            return Ok(());
        }
        let mut cmd = cmd;
        if ai::command_matches_danger(&cmd, &settings.danger_commands) {
            emit(&app, &session_id, "danger", "system", &cmd);
            emit(&app, &session_id, "wait-user", "system", "AI 申请执行危险命令，等待你处理");
            let reply = wait_for_danger(&hub, &session_id, &flag).await?;
            if reply.as_ref().map(|r| r.approved).unwrap_or(false) {
                if let Some(r) = reply {
                    if !r.command.trim().is_empty() {
                        cmd = r.command.trim().to_string();
                    }
                }
            } else {
                let note = "用户拒绝执行该危险命令。请把它当作否定答案，立刻寻找其他不涉及此类危险操作的最优方案，不要再提出同一命令。";
                history.push(("assistant".into(), serde_json::to_string(&step).unwrap_or_default()));
                history.push(("user".into(), note.into()));
                hub.save_history(&session_id, history.clone());
                continue;
            }
        }
        let cluster = hub.cluster_nodes(&session_id);
        let exec_id = if !cluster.is_empty() {
            step.node
                .as_ref()
                .and_then(|n| cluster.get(n).cloned())
                .or_else(|| cluster.values().next().cloned())
                .unwrap_or_else(|| session_id.clone())
        } else {
            session_id.clone()
        };
        ferra_core::oplog::write("ai", "command", &format!("session={exec_id} cmd={cmd}"));
        emit(&app, &session_id, "status", "system", &format!("执行：{cmd}"));
        emit(&app, &session_id, "waiting", "system", "等待终端输出");
        let output = match run_on_pty(&sessions, &exec_id, &cmd, &flag).await {
            Ok(o) => o,
            Err(e) if e == "interrupted" => {
                hub.save_history(&session_id, history);
                emit(&app, &session_id, "done", "system", "已打断");
                return Ok(());
            }
            Err(e) => {
                emit(&app, &session_id, "error", "system", &e);
                e
            }
        };
        emit(&app, &session_id, "waiting", "system", "");
        let clip: String = output.chars().rev().take(4000).collect::<String>().chars().rev().collect();
        history.push((
            "assistant".into(),
            serde_json::to_string(&LlmStep {
                say: step.say,
                command: Some(cmd),
                done: false,
                wait: None,
                wait_path: None,
                node: step.node,
            })
            .unwrap_or_default(),
        ));
        history.push((
            "user".into(),
            format!(
                "命令输出（节选）。出错则修复后继续原计划，成功则做下一步。只根据要点行动。\n{}",
                if clip.trim().is_empty() { "（无输出）" } else { &clip }
            ),
        ));
        hub.save_history(&session_id, history.clone());
    }
}

fn pty_text(sessions: &SessionManager, session_id: &str) -> String {
    sessions.get(session_id).map(|l| l.all_text()).unwrap_or_default()
}

fn last_nonempty_line(text: &str) -> &str {
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

fn waiting_for_term_input(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("[y/n]")
        || lower.contains("(yes/no)")
        || lower.contains("[yes/no]")
        || lower.contains("press return")
        || lower.contains("press enter")
}

fn at_shell_prompt(text: &str) -> bool {
    let line = last_nonempty_line(text);
    if line.is_empty() || line.len() > 240 || waiting_for_term_input(line) {
        return false;
    }
    matches!(
        line.chars().last(),
        Some('$') | Some('#') | Some('%') | Some('>') | Some('❯') | Some('➜') | Some('λ')
    )
}

async fn wait_for_user(
    hub: &AiHub,
    sessions: &SessionManager,
    session_id: &str,
    kind: &str,
    path: &str,
    flag: &AtomicBool,
) -> Result<String, String> {
    let start_completed = hub.upload_progress(session_id).0;
    let _ = hub.take_resume(session_id);
    let mut snap = pty_text(sessions, session_id);
    let mut last_change = Instant::now();
    let mut saw_term_activity = false;
    let mut user_note: Option<String> = None;
    loop {
        if stopped(flag) || hub.dropped(session_id) {
            return Ok("interrupted".into());
        }
        let inbox = hub.take_inbox(session_id);
        if !inbox.is_empty() {
            user_note = Some(format!("用户已补充信息，请继续原计划：{}", inbox.join("；")));
        }
        if hub.take_resume(session_id) {
            user_note = Some("用户确认已完成所需操作，请立刻按原计划继续执行，不要再重复询问。".into());
        }
        if user_note.is_none() && !path.is_empty() {
            if let Ok(live) = sessions.get(session_id) {
                if let Ok(sftp) = live.ensure_sftp().await {
                    if sftp::path_exists(&sftp, path).await {
                        user_note = Some(format!("已检测到远端路径存在：{path}。请继续原计划。"));
                    }
                }
            }
        }
        if user_note.is_none() && kind == "upload" {
            let (done, inflight) = hub.upload_progress(session_id);
            if done > start_completed && inflight == 0 {
                user_note = Some("本会话上传任务已全部完成。请立刻按原计划继续，不要再让用户重复上传。".into());
            }
        }

        let now = pty_text(sessions, session_id);
        if now != snap {
            saw_term_activity = true;
            snap = now.clone();
            last_change = Instant::now();
        }
        let quiet_prompt = last_change.elapsed() >= Duration::from_millis(1600);
        let quiet_fallback = last_change.elapsed() >= Duration::from_millis(4000);
        let idle = (quiet_prompt && at_shell_prompt(&now))
            || (quiet_fallback && user_note.is_some() && !waiting_for_term_input(last_nonempty_line(&now)));
        if idle && (user_note.is_some() || saw_term_activity) {
            if let Some(note) = user_note {
                return Ok(note);
            }
            return Ok(format!(
                "终端已回到空闲提示符，当前没有正在运行的任务。请根据最近输出继续原计划，不要再询问用户是否完成。\n{}",
                tail(&now, 2500)
            ));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

async fn wait_for_danger(
    hub: &AiHub,
    session_id: &str,
    flag: &AtomicBool,
) -> Result<Option<DangerReply>, String> {
    loop {
        if stopped(flag) || hub.dropped(session_id) {
            return Err("interrupted".into());
        }
        if let Some(r) = hub.take_danger_reply(session_id) {
            return Ok(Some(r));
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

async fn classify_continuity(
    client: &reqwest::Client,
    settings: &AiSettings,
    history: &[(String, String)],
    user_text: &str,
    flag: &AtomicBool,
) -> Result<bool, String> {
    let last = history
        .iter()
        .rev()
        .find(|(r, _)| r == "user")
        .map(|(_, t)| t.as_str())
        .unwrap_or("");
    let ask = format!(
        "判断新问题是否为上一问题的延续。只回答 CONTINUE 或 NEW。\n上一问：\n{last}\n新问：\n{user_text}"
    );
    let mini = vec![("user".into(), ask)];
    tokio::select! {
        _ = until_stopped(flag) => Err("interrupted".into()),
        raw = classify_plain(client, settings, &mini) => {
            let t = raw.unwrap_or_default().to_ascii_uppercase();
            Ok(t.contains("CONTINUE"))
        }
    }
}

async fn classify_plain(
    client: &reqwest::Client,
    settings: &AiSettings,
    history: &[(String, String)],
) -> Result<String, String> {
    let mut messages = vec![serde_json::json!({"role":"system","content": "只输出 CONTINUE 或 NEW。"})];
    for (role, content) in history {
        messages.push(serde_json::json!({"role": role, "content": content}));
    }
    let url = format!("{}/chat/completions", settings.resolve_base_url());
    if settings.provider == "anthropic" {
        return Ok("CONTINUE".into());
    }
    let mut req = client.post(url).json(&serde_json::json!({
        "model": settings.model,
        "temperature": 0,
        "max_tokens": 8,
        "messages": messages,
    }));
    if !settings.api_key.trim().is_empty() {
        req = req.bearer_auth(settings.api_key.trim());
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or("CONTINUE").to_string())
}

async fn run_on_pty(
    sessions: &SessionManager,
    session_id: &str,
    cmd: &str,
    flag: &AtomicBool,
) -> Result<String, String> {
    let live = sessions.get(session_id).map_err(map_err)?;
    let before = live.all_text();
    let mark = format!("{DONE_MARK}{}", Uuid::new_v4().simple());
    let line = format!("{cmd}; echo {mark}$?\n");
    live.write_bytes(line.into_bytes()).map_err(map_err)?;
    let download = looks_like_download(cmd);
    let limit = if download {
        Duration::from_secs(1800)
    } else {
        Duration::from_secs(180)
    };
    let start = Instant::now();
    let mut last = before.clone();
    let mut last_change = Instant::now();
    let mut done_chunk: Option<String> = None;
    loop {
        if stopped(flag) {
            let _ = live.write_bytes(vec![0x03]);
            return Err("interrupted".into());
        }
        tokio::time::sleep(Duration::from_millis(220)).await;
        let text = live.all_text();
        if text != last {
            last = text.clone();
            last_change = Instant::now();
        }
        if done_chunk.is_none() {
            if let Some(idx) = text.rfind(&mark) {
                if idx >= before.len() || !before.contains(&mark) {
                    let chunk = if text.len() > before.len() {
                        text[before.len().min(text.len())..].replace(&mark, "")
                    } else {
                        text[idx.saturating_sub(4000)..].replace(&mark, "")
                    };
                    done_chunk = Some(chunk.trim().to_string());
                    if !download {
                        return Ok(done_chunk.unwrap_or_default());
                    }
                }
            }
        }
        if let Some(chunk) = done_chunk.as_ref() {
            let quiet = last_change.elapsed() >= Duration::from_millis(1200);
            if quiet && at_shell_prompt(&text) {
                return Ok(chunk.clone());
            }
        }
        if start.elapsed() > limit {
            if let Some(chunk) = done_chunk {
                return Ok(chunk);
            }
            return Ok(format!("命令等待超时。最近终端内容：\n{}", tail(&text, 4000)));
        }
    }
}

fn looks_like_download(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "wget",
        "curl",
        "aria2c",
        "axel",
        "apt-get",
        "apt ",
        "yum ",
        "dnf ",
        "zypper",
        "pacman",
        "pip ",
        "pip3",
        "npm ",
        "pnpm",
        "yarn ",
        "scp ",
        "rsync",
        "sftp ",
        "git clone",
        "git fetch",
        "git pull",
        "docker pull",
        "podman pull",
        "cargo install",
    ];
    KEYS.iter().any(|k| c.contains(k))
}

fn tail(s: &str, n: usize) -> String {
    s.chars().rev().take(n).collect::<String>().chars().rev().collect()
}

async fn chat_llm_or_stop(
    client: &reqwest::Client,
    settings: &AiSettings,
    history: &[(String, String)],
    flag: &AtomicBool,
) -> Result<LlmStep, String> {
    tokio::select! {
        _ = until_stopped(flag) => Err("interrupted".into()),
        step = chat_llm(client, settings, history) => step,
    }
}

async fn chat_llm(client: &reqwest::Client, settings: &AiSettings, history: &[(String, String)]) -> Result<LlmStep, String> {
    let raw = if settings.provider == "anthropic" {
        anthropic_chat(client, settings, history).await?
    } else {
        openai_chat(client, settings, history).await?
    };
    ai::parse_llm_json(&raw).map_err(|e| format!("模型输出无法解析：{e}\n{raw}"))
}

async fn openai_chat(client: &reqwest::Client, settings: &AiSettings, history: &[(String, String)]) -> Result<String, String> {
    let mut messages = vec![serde_json::json!({"role":"system","content": ai::system_prompt()})];
    for (role, content) in history {
        messages.push(serde_json::json!({"role": role, "content": content}));
    }
    let url = format!("{}/chat/completions", settings.resolve_base_url());
    let mut req = client.post(url).json(&serde_json::json!({
        "model": settings.model,
        "temperature": 0.1,
        "messages": messages,
    }));
    if !settings.api_key.trim().is_empty() {
        req = req.bearer_auth(settings.api_key.trim());
    }
    let resp = req.send().await.map_err(|e| format!("调用模型失败：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("模型接口 {status}：{}", tail(&body, 800)));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| format!("接口返回非 JSON：{}", tail(&body, 400)))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| v["choices"][0]["text"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("接口没有 content：{}", tail(&body, 400)))
}

async fn anthropic_chat(client: &reqwest::Client, settings: &AiSettings, history: &[(String, String)]) -> Result<String, String> {
    let msgs: Vec<serde_json::Value> = history
        .iter()
        .map(|(role, content)| {
            let r = if role == "assistant" { "assistant" } else { "user" };
            serde_json::json!({"role": r, "content": content})
        })
        .collect();
    let url = format!("{}/v1/messages", settings.resolve_base_url().trim_end_matches("/v1"));
    let resp = client
        .post(url)
        .header("x-api-key", settings.api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": settings.model,
            "max_tokens": 2048,
            "temperature": 0.1,
            "system": ai::system_prompt(),
            "messages": msgs,
        }))
        .send()
        .await
        .map_err(|e| format!("调用 Claude 失败：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Claude 接口 {status}：{}", tail(&body, 800)));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| body.clone())?;
    v["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Claude 没有文本：{}", tail(&body, 400)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_idle_shell_prompt() {
        assert!(at_shell_prompt("root@host:/opt#"));
        assert!(at_shell_prompt("user@pc:~$"));
        assert!(!at_shell_prompt("Password:"));
        assert!(!at_shell_prompt("Continue? [y/n]"));
        assert!(!at_shell_prompt(""));
    }

    #[test]
    fn detects_download_commands() {
        assert!(looks_like_download("wget https://example.com/a.tar.gz"));
        assert!(looks_like_download("curl -L -o app.tgz https://example.com/app.tgz"));
        assert!(looks_like_download("apt-get install -y nginx"));
        assert!(looks_like_download("git clone https://github.com/foo/bar.git"));
        assert!(!looks_like_download("ls -la /opt"));
        assert!(!looks_like_download("systemctl status nginx"));
    }
}

