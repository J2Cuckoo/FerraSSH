import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "./api";
import TerminalView from "./Terminal";
import SftpPane from "./Sftp";
import MonitorPane from "./Monitor";
import Titlebar from "./Titlebar";
import AboutPage from "./About";
import SettingsPage from "./Settings";
import AiSettingsPage from "./AiSettings";
import AiPanel from "./AiPanel";
import ClusterPane from "./ClusterPane";
import SessionForm, { draftFromSession, emptySessionDraft, sessionFromDraft, type SessionDraft } from "./SessionForm";
import ContextMenu from "./ContextMenu";
import ToastHost from "./toast";
import PageLoading, { Spinner } from "./Loading";
import SealFail from "./SealFail";
import { resolveEyeCareFg } from "./termTheme";
import {
  emptyAlgs,
  type AppSettings,
  type ClusterProject,
  type Folder,
  type ProjectNode,
  type SavedKey,
  type SavedSession,
  type TermFrame,
  type TransferProgress,
} from "./types";

type Tab = { id: string; label: string; savedId?: string; frame: TermFrame | null; closed?: string };
type HostPrompt = {
  kind: "unknown" | "mismatch";
  host: string;
  port: number;
  fingerprint: string;
  expected?: string;
  retry: () => Promise<void>;
};

function invokeMessage(ex: unknown): string {
  if (typeof ex === "string") return ex;
  if (ex && typeof ex === "object" && "message" in ex) return String((ex as { message: unknown }).message);
  return String(ex);
}

function parseHostError(ex: unknown): {
  code?: string;
  message: string;
  host?: string;
  port?: number;
  fingerprint?: string;
  expected?: string;
} {
  const raw = invokeMessage(ex).replace(/^Error:\s*/i, "").trim();
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (parsed && typeof parsed.code === "string") {
      return {
        code: parsed.code,
        message: String(parsed.message ?? raw),
        host: typeof parsed.host === "string" ? parsed.host : undefined,
        port: typeof parsed.port === "number" ? parsed.port : undefined,
        fingerprint: typeof parsed.fingerprint === "string" ? parsed.fingerprint : undefined,
        expected: typeof parsed.expected === "string" ? parsed.expected : undefined,
      };
    }
  } catch {
    /* plain string from older russh abort */
  }
  const lower = raw.toLowerCase();
  if (lower.includes("unknown server key") || lower.includes("unknown host key")) {
    return { code: "unknown_host_key", message: raw };
  }
  if (lower.includes("host key mismatch")) {
    return { code: "host_key_mismatch", message: raw };
  }
  return { message: raw };
}

function hideUpdateUrls(s: string) {
  return s.replace(/https?:\/\/\S+/gi, "").replace(/[ \t]{2,}/g, " ").replace(/\n{3,}/g, "\n\n").trim();
}

function fmtUpdateBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function sessionCaption(s: Pick<SavedSession, "notes" | "username" | "host">) {
  return s.notes.trim() || `${s.username}@${s.host}`;
}

function sessionTabLabel(s: Pick<SavedSession, "name" | "notes" | "host" | "id">, tabs: { savedId?: string }[]) {
  const name = s.name.trim() || s.host.trim() || "会话";
  const remark = s.notes.trim().slice(0, 7);
  const base = remark ? `${name}@${remark}` : name;
  const n = tabs.filter((t) => t.savedId === s.id).length + 1;
  return n > 1 ? `${base} #${n}` : base;
}

function liveTabLabel(t: Tab, tabs: Tab[], sessions: SavedSession[]) {
  const s = t.savedId ? sessions.find((x) => x.id === t.savedId) : undefined;
  if (!s) return t.label;
  const name = s.name.trim() || s.host.trim() || "会话";
  const remark = s.notes.trim().slice(0, 7);
  const base = remark ? `${name}@${remark}` : name;
  const siblings = tabs.filter((x) => x.savedId === t.savedId);
  const n = siblings.findIndex((x) => x.id === t.id) + 1;
  return siblings.length > 1 ? `${base} #${n}` : base;
}

function EllipsisHint({ text }: { text: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [overflow, setOverflow] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setOverflow(el.scrollWidth > el.clientWidth + 1);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  }, [text]);

  return (
    <span ref={ref} className="sess-info-val" title={overflow ? text : undefined}>
      {text || "—"}
    </span>
  );
}

function DisconnectIcon() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <rect x="5.5" y="5.5" width="5" height="5" rx="0.7" fill="currentColor" />
    </svg>
  );
}

function ConnectIcon() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M6.5 5.1v5.8L11.4 8z" fill="currentColor" />
    </svg>
  );
}

function CopyIcon() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <rect x="5.2" y="5.2" width="7.6" height="7.6" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M3.4 10.4V4.2A1.2 1.2 0 0 1 4.6 3h6.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}


function CopyableValue({ text }: { text: string }) {
  const [ok, setOk] = useState(false);
  const value = text.trim();

  return (
    <span className="sess-info-copywrap">
      <span className="sess-info-val">{value || "—"}</span>
      {value ? (
        <button
          type="button"
          className={`sess-info-copy${ok ? " ok" : ""}`}
          title={ok ? "已复制" : "复制"}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            void api.clipboardWrite(value).then(() => {
              setOk(true);
              window.setTimeout(() => setOk(false), 1200);
            }).catch(() => {});
          }}
        >
          <CopyIcon />
        </button>
      ) : null}
    </span>
  );
}

type TreeDrag = { kind: "session" | "folder" | "project" | "node"; id: string; extra?: string };

function folderChildren(folders: Folder[], parentId: string | null): Folder[] {
  const ids = new Set(folders.map((f) => f.id));
  return folders
    .filter((f) => {
      const p = f.parent_id;
      if (parentId === null) return !p || !ids.has(p);
      return p === parentId;
    })
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name, "zh"));
}

function walkFolders(folders: Folder[], parentId: string | null = null, depth = 0): { folder: Folder; depth: number }[] {
  return folderChildren(folders, parentId).flatMap((folder) => [
    { folder, depth },
    ...walkFolders(folders, folder.id, depth + 1),
  ]);
}

function folderIsSelfOrDescendant(folders: Folder[], ancestorId: string, nodeId: string): boolean {
  if (ancestorId === nodeId) return true;
  const seen = new Set<string>();
  let cur = folders.find((f) => f.id === nodeId);
  while (cur?.parent_id) {
    const parentId = cur.parent_id;
    if (parentId === ancestorId) return true;
    if (seen.has(cur.id)) break;
    seen.add(cur.id);
    cur = folders.find((f) => f.id === parentId);
  }
  return false;
}

function folderPath(folders: Folder[], id: string): string {
  const names: string[] = [];
  const seen = new Set<string>();
  let cur = folders.find((f) => f.id === id);
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    names.unshift(cur.name);
    cur = cur.parent_id ? folders.find((f) => f.id === cur!.parent_id) : undefined;
  }
  return names.join(" / ");
}

export default function App() {
  const [gate, setGate] = useState<"seal" | "setup" | "lock" | "app">("lock");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState("");
  const [sessions, setSessions] = useState<SavedSession[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [keys, setKeys] = useState<SavedKey[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const tabStripRef = useRef<HTMLDivElement>(null);
  const [jobs, setJobs] = useState<TransferProgress[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [keepNew, setKeepNew] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [newForCluster, setNewForCluster] = useState(false);
  const [workspace, setWorkspace] = useState<"ops" | "cluster">("ops");
  const [projects, setProjects] = useState<ClusterProject[]>([]);
  const [projectNodes, setProjectNodes] = useState<ProjectNode[]>([]);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const [clusterOverview, setClusterOverview] = useState(true);
  const [showSet, setShowSet] = useState(false);
  const [showAi, setShowAi] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [aiOn, setAiOn] = useState(false);
  const [aiDockW, setAiDockW] = useState(320);
  const [showUpdate, setShowUpdate] = useState(false);
  const [keepSet, setKeepSet] = useState(false);
  const [keepAi, setKeepAi] = useState(false);
  const [keepAbout, setKeepAbout] = useState(false);
  const [updateText, setUpdateText] = useState("");
  const [updateInfo, setUpdateInfo] = useState<{
    current: string;
    latest: string;
    newer: boolean;
    notes: string;
  } | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateProg, setUpdateProg] = useState<{ stage: string; transferred: number; total: number } | null>(null);
  const [hostPrompt, setHostPrompt] = useState<HostPrompt | null>(null);
  const [side, setSide] = useState<"term" | "sftp" | "monitor">("term");
  const [sideNonce, setSideNonce] = useState(0);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [dropId, setDropId] = useState<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [ghost, setGhost] = useState<{ x: number; y: number; label: string } | null>(null);
  const foldersRef = useRef<Folder[]>([]);
  const sessionsRef = useRef<SavedSession[]>([]);
  const dropIdRef = useRef<string | null>(null);
  const skipFolderClick = useRef(false);
  const skipSessionClick = useRef(false);
  const connectingIds = useRef(new Set<string>());
  const [inspectedId, setInspectedId] = useState<string | null>(null);
  foldersRef.current = folders;
  sessionsRef.current = sessions;
  const [folderDlg, setFolderDlg] = useState<{ parentId: string | null } | null>(null);
  const [folderName, setFolderName] = useState("");
  const [projectDlg, setProjectDlg] = useState(false);
  const [projectName, setProjectName] = useState("");
  const [pickNode, setPickNode] = useState(false);
  const [ctx, setCtx] = useState<{ x: number; y: number; sessionId: string } | null>(null);
  const [blankCtx, setBlankCtx] = useState<{ x: number; y: number } | null>(null);
  const [danger, setDanger] = useState<{ sessionId: string; command: string } | null>(null);
  const [dangerCmd, setDangerCmd] = useState("");
  const [unlocking, setUnlocking] = useState(false);
  const [pageLoad, setPageLoad] = useState(false);
  const pageLoadGen = useRef(0);
  const [draft, setDraft] = useState<SessionDraft>(emptySessionDraft());

  async function boot() {
    try {
      const sealed = await api.deviceSealOk().catch(() => false);
      if (!sealed) {
        setGate("seal");
        return;
      }
      try {
        const loaded = await api.settings();
        setSettings({ ...loaded, theme: loaded.theme || "ink", term_fg: loaded.term_fg || "#d6deeb" });
      } catch {
        /* vault may not exist yet */
      }
      const s = await api.vaultStatus();
      setGate(s.unlocked ? "app" : s.initialized ? "lock" : "setup");
      if (s.unlocked) await loadApp();
    } catch (e) {
      setErr(String(e));
    }
  }

  async function loadApp() {
    setSessions(await api.listSessions());
    setFolders(await api.listFolders());
    setKeys(await api.listKeys());
    try {
      setProjects(await api.listProjects());
      setProjectNodes(await api.listProjectNodes());
    } catch {
      setProjects([]);
      setProjectNodes([]);
    }
    const loaded = await api.settings();
    setSettings({
      ...loaded,
      theme: loaded.theme || "ink",
      term_fg: loaded.term_fg || "#d6deeb",
      log_retain_days: loaded.log_retain_days ?? 5,
    });
    try {
      const ai = await api.aiSettings();
      setAiOn(!!ai.enabled);
    } catch {
      setAiOn(false);
    }
    setExpanded({});
    setGate("app");
    void quietCheckUpdate();
  }

  useEffect(() => {
    boot();
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme || "ink";
  }, [settings?.theme]);

  useEffect(() => {
    const un = [
      listen<{ session_id: string; frame: TermFrame }>("term-frame", (e) => {
        setTabs((tabs) =>
          tabs.map((t) => (t.id === e.payload.session_id ? { ...t, frame: e.payload.frame } : t)),
        );
      }),
      listen<{ session_id: string; message: string }>("term-closed", (e) => {
        setTabs((tabs) => tabs.map((t) => (t.id === e.payload.session_id ? { ...t, closed: e.payload.message } : t)));
      }),
      listen<{ session_id: string; text: string }>("term-clipboard", (e) => {
        void api.clipboardWrite(e.payload.text).catch(() => {});
      }),
      listen<TransferProgress>("sftp-progress", (e) => {
        setJobs((js) => {
          const rest = js.filter((j) => !(j.job_id === e.payload.job_id && j.path === e.payload.path));
          return [...rest, e.payload].slice(-80);
        });
      }),
      listen<{ stage: string; transferred: number; total: number }>("app-update-progress", (e) => {
        setUpdateProg(e.payload);
        if (e.payload.stage === "install") {
          setUpdateText("下载完成，正在安装，完成后会自动打开。");
        }
      }),
      listen<{ session_id: string; kind: string; text: string }>("ai-event", (e) => {
        if (e.payload.kind !== "danger") return;
        setDanger({ sessionId: e.payload.session_id, command: e.payload.text });
        setDangerCmd(e.payload.text);
      }),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, []);

  async function withPageLoad<T>(fn: () => Promise<T>): Promise<T | undefined> {
    const gen = ++pageLoadGen.current;
    setPageLoad(true);
    try {
      return await fn();
    } finally {
      if (pageLoadGen.current === gen) setPageLoad(false);
    }
  }

  async function unlock(e: React.FormEvent) {
    e.preventDefault();
    if (unlocking) return;
    setErr("");
    setUnlocking(true);
    await new Promise((r) => window.setTimeout(r, 0));
    try {
      if (gate === "setup") {
        if (pw.length < 8) throw new Error("主密码至少 8 位");
        if (pw !== pw2) throw new Error("两次密码不一致");
        await api.vaultInit(pw);
      } else {
        await api.vaultUnlock(pw);
      }
      setPw("");
      setPw2("");
      await loadApp();
    } catch (ex) {
      setErr(String(ex));
    } finally {
      setUnlocking(false);
    }
  }

  async function quietCheckUpdate() {
    const current = await getVersion().catch(() => "0.2.10");
    try {
      const info = await api.checkAppUpdate(current);
      if (!info.newer) return;
      setUpdateInfo({ ...info, notes: hideUpdateUrls(info.notes || "") });
      setUpdateText(`发现新版本 ${info.latest}（当前 ${info.current}）。`);
      setShowUpdate(true);
    } catch {
      /* 启动时检查失败不打扰 */
    }
  }

  async function checkUpdate() {
    setShowUpdate(true);
    setUpdateBusy(false);
    setUpdateProg(null);
    setUpdateInfo(null);
    setUpdateText("正在检查更新…");
    const current = await getVersion().catch(() => "0.2.10");
    try {
      const info = await api.checkAppUpdate(current);
      setUpdateInfo({ ...info, notes: hideUpdateUrls(info.notes || "") });
      if (info.newer) {
        setUpdateText(`发现新版本 ${info.latest}（当前 ${info.current}）。`);
      } else {
        setUpdateText(`当前已是最新版本 ${current}。`);
      }
    } catch (e) {
      setUpdateInfo(null);
      setUpdateText(`当前版本 ${current}。检查更新失败：${hideUpdateUrls(String(e))}`);
    }
  }

  async function applyUpdate() {
    if (!updateInfo?.newer || updateBusy) return;
    setUpdateBusy(true);
    setUpdateProg({ stage: "download", transferred: 0, total: 0 });
    setUpdateText("正在下载更新包…");
    const current = await getVersion().catch(() => "0.2.10");
    try {
      await api.installAppUpdate(current);
    } catch (e) {
      setUpdateBusy(false);
      setUpdateProg(null);
      setUpdateText(`更新失败：${hideUpdateUrls(String(e))}`);
    }
  }

  function goHome() {
    setShowAbout(false);
    setShowSet(false);
    setShowAi(false);
    setShowUpdate(false);
    setShowNew(false);
    setHostPrompt(null);
    setFolderDlg(null);
    setCtx(null);
    setBlankCtx(null);
    setPickNode(false);
    setProjectDlg(false);
  }

  const chrome = (
    <Titlebar
      settingsEnabled={gate === "app"}
      settingsActive={showSet}
      aiActive={showAi}
      aboutActive={showAbout}
      onHome={goHome}
      onSettings={() => {
        if (gate !== "app") return;
        setShowAbout(false);
        setShowAi(false);
        setShowNew(false);
        setKeepSet(true);
        setShowSet((open) => !open);
      }}
      onAi={() => {
        if (gate !== "app") return;
        setShowAbout(false);
        setShowSet(false);
        setShowNew(false);
        setKeepAi(true);
        setShowAi((open) => !open);
      }}
      onAbout={() => {
        setShowSet(false);
        setShowAi(false);
        setKeepAbout(true);
        setShowAbout((open) => !open);
      }}
      onUpdate={() => void checkUpdate()}
    />
  );

  async function openSaved(s: SavedSession, acceptUnknown = false) {
    if (connectingIds.current.has(s.id)) return;
    connectingIds.current.add(s.id);
    setErr("");
    await withPageLoad(async () => {
      try {
        const r = await api.connectSaved(s.id, 120, 32, acceptUnknown);
        setHostPrompt(null);
        setTabs((t) => [...t, { id: r.session_id, label: sessionTabLabel(s, t), savedId: s.id, frame: null }]);
        setActive(r.session_id);
        setSide("term");
        setClusterOverview(false);
      } catch (ex) {
        const info = parseHostError(ex);
        if (info.code === "unknown_host_key" || info.code === "host_key_mismatch") {
          setHostPrompt({
            kind: info.code === "host_key_mismatch" ? "mismatch" : "unknown",
            host: info.host || s.host,
            port: info.port || s.port,
            fingerprint: info.fingerprint || "",
            expected: info.expected,
            retry: () => openSaved(s, true),
          });
        } else {
          setErr(info.message);
        }
      } finally {
        connectingIds.current.delete(s.id);
      }
    });
  }

  function openNewSession(forCluster = false) {
    setDraft({ ...emptySessionDraft(), in_ops: !forCluster || true, folder_id: "" });
    if (forCluster) setDraft((d) => ({ ...d, in_ops: true }));
    setEditingId(null);
    setNewForCluster(forCluster);
    setShowSet(false);
    setShowAi(false);
    setShowAbout(false);
    setKeepNew(true);
    setShowNew(true);
    setErr("");
  }

  function openEditSession(s: SavedSession) {
    setDraft(draftFromSession(s));
    setEditingId(s.id);
    setNewForCluster(false);
    setKeepNew(true);
    setShowNew(true);
    setCtx(null);
    setErr("");
  }

  async function saveDraft(andConnect: boolean) {
    setErr("");
    try {
      const existing = editingId ? sessions.find((s) => s.id === editingId) : undefined;
      const id = editingId || crypto.randomUUID();
      const session = sessionFromDraft(draft, id, sessions.length, existing);
      if (newForCluster && !draft.in_ops) session.in_ops = false;
      if (settings?.default_term) session.term = existing?.term || settings.default_term;
      await api.upsertSession(session, draft.password || undefined, draft.passphrase || undefined);
      void api.oplogWrite("human", editingId ? "session-edit" : "session-create", session.name);
      setSessions(await api.listSessions());
      if (newForCluster && selectedProject) {
        const n = projectNodes.filter((x) => x.project_id === selectedProject).length;
        await api.addProjectNode(selectedProject, session.id, n);
        setProjectNodes(await api.listProjectNodes());
      }
      const wasEdit = !!editingId;
      const wasCluster = newForCluster;
      setDraft(emptySessionDraft());
      setEditingId(null);
      setNewForCluster(false);
      if (wasEdit || wasCluster || andConnect) setShowNew(false);
      if (andConnect && !wasEdit && !wasCluster) await openSaved(session);
    } catch (ex) {
      setErr(parseHostError(ex).message);
    }
  }

  async function quickConnect() {
    setErr("");
    await withPageLoad(async () => {
      try {
        const r = await api.connectQuick({
          host: draft.host,
          port: Number(draft.port) || 22,
          username: draft.username,
          password: draft.password || null,
          passphrase: draft.passphrase || null,
          private_key_pem: null,
          key_id: draft.auth_method === "key" && draft.key_id ? draft.key_id : null,
          use_agent: draft.auth_method === "agent",
          profile: { ...emptyAlgs(), profile: draft.profile },
          accept_unknown_host: true,
          local_echo: draft.local_echo,
          term: settings?.default_term || "xterm-256color",
          cols: 120,
          rows: 32,
        });
        setTabs((t) => [
          ...t,
          {
            id: r.session_id,
            label: sessionTabLabel(
              { id: "quick", name: draft.name, notes: draft.notes, host: draft.host },
              t,
            ),
            frame: null,
          },
        ]);
        setActive(r.session_id);
        setSide("term");
        setClusterOverview(false);
        setShowNew(false);
        setDraft(emptySessionDraft());
      } catch (ex) {
        setErr(String(ex));
      }
    });
  }

  async function removeSession(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    if (!confirm("删除该会话配置？")) return;
    await api.deleteSession(id);
    setSessions(await api.listSessions());
    setInspectedId((cur) => (cur === id ? null : cur));
  }

  function hitDropZone(x: number, y: number, item: TreeDrag): string | null {
    const stack = document.elementsFromPoint(x, y);
    for (const node of stack) {
      if (!(node instanceof HTMLElement)) continue;
      if (node.closest(".tree-ghost")) continue;
      const zone = node.closest("[data-drop]");
      if (!(zone instanceof HTMLElement)) continue;
      const raw = zone.dataset.drop ?? "";
      if (!raw) continue;
      if (item.kind === "folder") {
        if (raw === item.id) continue;
        if (raw !== "root" && folderIsSelfOrDescendant(foldersRef.current, item.id, raw)) continue;
      }
      return raw;
    }
    return null;
  }

  async function moveFolder(folderId: string, parentId: string | null) {
    const list = foldersRef.current;
    if (parentId && folderIsSelfOrDescendant(list, folderId, parentId)) return;
    const f = list.find((x) => x.id === folderId);
    if (!f) return;
    if ((f.parent_id ?? null) === parentId) return;
    await api.upsertFolder({
      ...f,
      parent_id: parentId,
      sort_order: folderChildren(list, parentId).length,
    });
    setFolders(await api.listFolders());
    if (parentId) setExpanded((x) => ({ ...x, [parentId]: true }));
  }

  async function commitMove(item: TreeDrag, target: string | null) {
    if (!target) return;
    if (item.kind === "session") {
      const folderId = target === "root" ? null : target.startsWith("project:") ? null : target;
      const s = sessionsRef.current.find((x) => x.id === item.id);
      if (!s) return;
      if ((s.folder_id ?? null) === folderId) return;
      await api.upsertSession({ ...s, folder_id: folderId });
      setSessions(await api.listSessions());
      if (folderId) setExpanded((x) => ({ ...x, [folderId]: true }));
      return;
    }
    if (item.kind === "project") {
      const folderId = target === "root" || target.startsWith("project:") ? null : target;
      const p = projects.find((x) => x.id === item.id);
      if (!p) return;
      if ((p.folder_id ?? null) === folderId) return;
      await api.upsertProject({ ...p, folder_id: folderId });
      setProjects(await api.listProjects());
      if (folderId) setExpanded((x) => ({ ...x, [folderId]: true }));
      return;
    }
    if (item.kind === "node") {
      const projectId = target.startsWith("project:") ? target.slice(8) : target;
      if (!projectId || projectId === "root") return;
      const from = item.extra;
      if (from === projectId) return;
      if (from) await api.removeProjectNode(from, item.id);
      const n = projectNodes.filter((x) => x.project_id === projectId).length;
      await api.addProjectNode(projectId, item.id, n);
      setProjectNodes(await api.listProjectNodes());
      setExpanded((x) => ({ ...x, [projectId]: true }));
      return;
    }
    await moveFolder(item.id, target === "root" || target.startsWith("project:") ? null : target);
  }

  function beginMove(e: React.PointerEvent, item: TreeDrag, label: string) {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    const originX = e.clientX;
    const originY = e.clientY;
    let started = false;

    const onMove = (ev: PointerEvent) => {
      if (!started) {
        if (Math.hypot(ev.clientX - originX, ev.clientY - originY) < 6) return;
        started = true;
        setDragId(item.id);
        document.body.classList.add("tree-dragging");
      }
      ev.preventDefault();
      const next = hitDropZone(ev.clientX, ev.clientY, item);
      dropIdRef.current = next;
      setDropId(next);
      setGhost({ x: ev.clientX, y: ev.clientY, label });
    };

    const finish = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      document.body.classList.remove("tree-dragging");
      const target = dropIdRef.current;
      const did = started;
      setGhost(null);
      setDragId(null);
      setDropId(null);
      dropIdRef.current = null;
      if (!did) return;
      skipFolderClick.current = true;
      skipSessionClick.current = true;
      void commitMove(item, target);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  }

  function addFolder(parentId: string | null = null) {
    setFolderName("");
    setFolderDlg({ parentId });
  }

  async function submitFolder(e: React.FormEvent) {
    e.preventDefault();
    const name = folderName.trim();
    if (!name || !folderDlg) return;
    const parentId = folderDlg.parentId;
    await api.upsertFolder({
      id: crypto.randomUUID(),
      parent_id: parentId,
      name,
      sort_order: folderChildren(folders.filter((f) => (f.kind || "ops") === workspace), parentId).length,
      kind: workspace,
    });
    setFolders(await api.listFolders());
    if (parentId) setExpanded((x) => ({ ...x, [parentId]: true }));
    setFolderDlg(null);
    setFolderName("");
  }

  async function removeFolder(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    if (!confirm("删除该分组？其中的会话会移到未分组，子分组会提到上一层。")) return;
    await api.deleteFolder(id);
    setFolders(await api.listFolders());
    setSessions(await api.listSessions());
  }

  async function moveSession(sessionId: string, folderId: string | null) {
    const s = sessions.find((x) => x.id === sessionId);
    if (!s) return;
    await api.upsertSession({ ...s, folder_id: folderId });
    setSessions(await api.listSessions());
    setCtx(null);
  }

  const currentId =
    workspace === "cluster" && clusterOverview
      ? null
      : (active && tabs.some((t) => t.id === active) ? active : tabs[0]?.id) ?? null;
  const tab = tabs.find((t) => t.id === currentId);
  const tabIds = tabs.map((t) => t.id).join("\0");

  useEffect(() => {
    const strip = tabStripRef.current;
    if (!strip) return;

    const pinActiveLeft = () => {
      if (!currentId) {
        strip.scrollLeft = 0;
        return;
      }
      const el = Array.from(strip.children).find(
        (node) => node instanceof HTMLElement && node.dataset.tabId === currentId,
      ) as HTMLElement | undefined;
      if (!el) return;
      if (strip.scrollWidth <= strip.clientWidth + 1) {
        strip.scrollLeft = 0;
        return;
      }
      const box = strip.getBoundingClientRect();
      const tabBox = el.getBoundingClientRect();
      strip.scrollTo({ left: strip.scrollLeft + (tabBox.left - box.left), behavior: "auto" });
    };

    pinActiveLeft();
    const onWheel = (e: WheelEvent) => {
      if (strip.scrollWidth <= strip.clientWidth) return;
      const delta = e.deltaX !== 0 ? e.deltaX : e.deltaY;
      if (delta === 0) return;
      e.preventDefault();
      strip.scrollLeft += delta;
    };
    strip.addEventListener("wheel", onWheel, { passive: false });
    const ro = new ResizeObserver(pinActiveLeft);
    ro.observe(strip);
    return () => {
      strip.removeEventListener("wheel", onWheel);
      ro.disconnect();
    };
  }, [currentId, tabIds, gate]);

  const activeSavedId = tab?.savedId;
  const liveIds = new Set(tabs.filter((t) => t.savedId && !t.closed).map((t) => t.savedId as string));
  const kindFolders = folders.filter((f) => (f.kind || "ops") === workspace);
  const opsSessions = sessions.filter((s) => s.in_ops !== false);
  const ungrouped = opsSessions.filter((s) => !s.folder_id);
  const inspected = inspectedId
    ? (workspace === "ops" ? opsSessions : sessions).find((s) => s.id === inspectedId)
    : undefined;
  const liveMap: Record<string, string> = {};
  for (const t of tabs) {
    if (t.savedId && !t.closed) liveMap[t.savedId] = t.id;
  }
  const currentProject = selectedProject ? projects.find((p) => p.id === selectedProject) : undefined;
  const currentProjectNodes = selectedProject
    ? projectNodes
        .filter((n) => n.project_id === selectedProject)
        .map((n) => sessions.find((s) => s.id === n.session_id))
        .filter((s): s is SavedSession => !!s)
    : [];

  function closeTabs(ids: string[]) {
    if (ids.length === 0) return;
    const closing = new Set(ids);
    for (const id of ids) {
      void api.aiCancel(id).catch(() => {});
      void api.disconnect(id);
    }
    const idx = currentId ? tabs.findIndex((t) => t.id === currentId) : -1;
    const remaining = tabs.filter((t) => !closing.has(t.id));
    setTabs(remaining);
    if (currentId && closing.has(currentId)) {
      setActive((remaining[Math.max(0, idx - 1)] ?? remaining[0])?.id ?? null);
    }
  }

  function toggleSessionConn(s: SavedSession) {
    const live = tabs.filter((t) => t.savedId === s.id && !t.closed);
    if (live.length) closeTabs(live.map((t) => t.id));
    else void openSaved(s);
  }

  function sessionConnBtn(s: SavedSession) {
    const live = liveIds.has(s.id);
    return (
      <button
        type="button"
        className={`sess-conn${live ? " on" : ""}`}
        title={live ? "断开连接" : "连接"}
        onPointerDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          toggleSessionConn(s);
        }}
      >
        {live ? <DisconnectIcon /> : <ConnectIcon />}
      </button>
    );
  }

  function renderSession(s: SavedSession) {
    const on = activeSavedId === s.id;
    const live = liveIds.has(s.id);
    return (
      <li
        key={s.id}
        className={`${on ? "on" : ""} ${live ? "live" : ""} ${dragId === s.id ? "dragging" : ""}`}
        onPointerDown={(e) => beginMove(e, { kind: "session", id: s.id }, s.name || s.host)}
        onClick={() => {
          if (skipSessionClick.current) {
            skipSessionClick.current = false;
            return;
          }
          setInspectedId(s.id);
        }}
        onDoubleClick={() => openSaved(s)}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setBlankCtx(null);
          setCtx({ x: e.clientX, y: e.clientY, sessionId: s.id });
        }}
        title={`${s.username}@${s.host}:${s.port}`}
      >
        <span className="tree-toggle" aria-hidden />
        <span className="tree-ico">
          <span className={`logo sm ${live ? "pulse" : ""}`} aria-hidden>
            Fs
          </span>
        </span>
        <div>
          <b>{s.name || s.host}</b>
          <small>{sessionCaption(s)}</small>
        </div>
        {sessionConnBtn(s)}
        <button className="icon-btn" title="删除" onClick={(e) => removeSession(s.id, e)}>
          ×
        </button>
      </li>
    );
  }

  function renderNode(s: SavedSession, projectId: string) {
    const live = liveIds.has(s.id);
    return (
      <li
        key={`node-${s.id}`}
        className={`${live ? "live" : ""} ${dragId === s.id ? "dragging" : ""}`}
        onPointerDown={(e) => beginMove(e, { kind: "node", id: s.id, extra: projectId }, s.name || s.host)}
        onClick={() => setInspectedId(s.id)}
        onDoubleClick={() => openSaved(s)}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setCtx({ x: e.clientX, y: e.clientY, sessionId: s.id });
        }}
        title={`${s.username}@${s.host}:${s.port}`}
      >
        <span className="tree-toggle" aria-hidden />
        <span className="tree-ico">
          <span className={`logo sm ${live ? "pulse" : ""}`}>Fs</span>
        </span>
        <div>
          <b>{s.name || s.host}</b>
          <small>{sessionCaption(s)}</small>
        </div>
        {sessionConnBtn(s)}
      </li>
    );
  }

  function renderProject(p: ClusterProject) {
    const open = !!expanded[p.id] || selectedProject === p.id;
    const nodes = projectNodes
      .filter((n) => n.project_id === p.id)
      .map((n) => sessions.find((s) => s.id === n.session_id))
      .filter((s): s is SavedSession => !!s);
    return (
      <li key={p.id} className={`folder ${dragId === p.id ? "dragging" : ""}`}>
        <div
          className={`folder-head ${selectedProject === p.id ? "on" : ""} ${dropId === `project:${p.id}` ? "drop" : ""}`}
          data-drop={`project:${p.id}`}
          onPointerDown={(e) => beginMove(e, { kind: "project", id: p.id }, p.name)}
          onClick={() => {
            if (skipFolderClick.current) {
              skipFolderClick.current = false;
              return;
            }
            setSelectedProject(p.id);
            setClusterOverview(true);
            setExpanded((x) => ({ ...x, [p.id]: true }));
          }}
        >
          <span className="tree-toggle chev">{open ? "▾" : "▸"}</span>
          <span className="tree-ico">
            <svg className="folder-ico" viewBox="0 0 16 16" aria-hidden>
              <path d="M2 3.5h5.2l.8 1.2H14A1.5 1.5 0 0 1 15.5 6.2v6.3A1.5 1.5 0 0 1 14 14H2A1.5 1.5 0 0 1 .5 12.5v-8A1.5 1.5 0 0 1 2 3.5z" />
            </svg>
          </span>
          <b>{p.name}</b>
          <button
            className="icon-btn"
            title="删除项目"
            onClick={(e) => {
              e.stopPropagation();
              if (!confirm("删除该项目？节点连接不会从常规运维删除。")) return;
              void api.deleteProject(p.id).then(async () => {
                setProjects(await api.listProjects());
                setProjectNodes(await api.listProjectNodes());
                if (selectedProject === p.id) setSelectedProject(null);
              });
            }}
          >
            ×
          </button>
        </div>
        {open && (
          <ul className="sess nested" data-drop={`project:${p.id}`}>
            {nodes.map((s) => renderNode(s, p.id))}
          </ul>
        )}
      </li>
    );
  }

  function renderFolder(f: Folder) {
    const open = !!expanded[f.id];
    const childFolders = folderChildren(kindFolders, f.id);
    const kids =
      workspace === "ops"
        ? opsSessions.filter((s) => s.folder_id === f.id)
        : projects.filter((p) => p.folder_id === f.id);
    return (
      <li key={f.id} className={`folder ${dragId === f.id ? "dragging" : ""}`}>
        <div
          className={`folder-head ${dropId === f.id ? "drop" : ""}`}
          data-drop={f.id}
          onPointerDown={(e) => beginMove(e, { kind: "folder", id: f.id }, f.name)}
          onClick={() => {
            if (skipFolderClick.current) {
              skipFolderClick.current = false;
              return;
            }
            setExpanded((x) => ({ ...x, [f.id]: !x[f.id] }));
          }}
        >
          <span className="tree-toggle chev">{open ? "▾" : "▸"}</span>
          <span className="tree-ico">
            <svg className="folder-ico" viewBox="0 0 16 16" aria-hidden>
              <path d="M1.5 3.75A1.25 1.25 0 0 1 2.75 2.5h3.1c.3 0 .58.14.76.37l.72.9c.18.23.46.37.76.37h5.16A1.25 1.25 0 0 1 14.5 5.4v6.85A1.25 1.25 0 0 1 13.25 13.5H2.75A1.25 1.25 0 0 1 1.5 12.25V3.75z" />
            </svg>
          </span>
          <b>{f.name}</b>
          <button
            className="icon-btn add"
            title="新建子分组"
            onClick={(e) => {
              e.stopPropagation();
              void addFolder(f.id);
            }}
          >
            +
          </button>
          <button className="icon-btn" title="删除分组" onClick={(e) => removeFolder(f.id, e)}>
            ×
          </button>
        </div>
        {open && (
          <ul className="sess nested" data-drop={f.id}>
            {childFolders.map(renderFolder)}
            {workspace === "ops" ? kids.map((s) => renderSession(s as SavedSession)) : (kids as ClusterProject[]).map(renderProject)}
          </ul>
        )}
      </li>
    );
  }

  const chromeDialogs = (
    <>
      {showUpdate && (
        <div
          className="modal"
          onClick={() => {
            if (!updateBusy) setShowUpdate(false);
          }}
        >
          <div className="card" onClick={(e) => e.stopPropagation()}>
            <h2>更新</h2>
            <p className="hint">{hideUpdateUrls(updateText)}</p>
            {updateInfo?.newer && updateInfo.notes ? (
              <pre className="update-notes">{updateInfo.notes}</pre>
            ) : null}
            {updateBusy && (
              <div className="update-xfer">
                <progress
                  value={updateProg && updateProg.total > 0 ? updateProg.transferred : undefined}
                  max={updateProg && updateProg.total > 0 ? updateProg.total : undefined}
                />
                <span className="muted">
                  {updateProg?.stage === "install"
                    ? "下载完成，正在安装，完成后会自动打开。"
                    : updateProg && updateProg.total > 0
                      ? `已下载 ${fmtUpdateBytes(updateProg.transferred)} / ${fmtUpdateBytes(updateProg.total)}`
                      : "正在下载更新包…"}
                </span>
              </div>
            )}
            <div className="row card-actions">
              {updateInfo?.newer ? (
                <button type="button" className="primary" disabled={updateBusy} onClick={() => void applyUpdate()}>
                  {updateBusy ? "正在更新…" : "确认更新"}
                </button>
              ) : null}
              <button type="button" disabled={updateBusy} onClick={() => setShowUpdate(false)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );

  function closeAbout() {
    setShowAbout(false);
  }

  if (gate === "seal") {
    return <SealFail />;
  }

  if (gate === "setup" || gate === "lock") {
    return (
      <div className="app-frame">
        {chrome}
        <div className="app-body">
          {pageLoad && <PageLoading />}
          <div className="gate">
              <div className="card">
                <div className="brand">
                  <span className="logo">Fs</span>
                  <div>
                    <h1>FerraSSH</h1>
                    <p>主密码保护的 SSH / SFTP 工作站</p>
                  </div>
                </div>
                <form onSubmit={unlock}>
                  <label>
                    主密码
                    <input
                      type="password"
                      value={pw}
                      onChange={(e) => setPw(e.target.value)}
                      autoFocus
                      disabled={unlocking}
                    />
                  </label>
                  {gate === "setup" && (
                    <label>
                      确认主密码
                      <input
                        type="password"
                        value={pw2}
                        onChange={(e) => setPw2(e.target.value)}
                        disabled={unlocking}
                      />
                    </label>
                  )}
                  {err && <div className="error">{err}</div>}
                  <button type="submit" className="btn-wait" disabled={unlocking}>
                    {unlocking ? (
                      <>
                        <Spinner />
                        {gate === "setup" ? "正在创建…" : "正在解锁…"}
                      </>
                    ) : gate === "setup" ? (
                      "创建保险库"
                    ) : (
                      "解锁"
                    )}
                  </button>
                  <p className="hint">连接从容，如在本地。</p>
                </form>
              </div>
            </div>
          {keepAbout && (
            <div className="page-overlay" hidden={!showAbout}>
              <AboutPage onClose={closeAbout} />
            </div>
          )}
        </div>
        {chromeDialogs}
      </div>
    );
  }

  return (
    <div className="app-frame" onClick={() => { setCtx(null); setBlankCtx(null); }}>
      {chrome}
      <div className="app-body">
      {pageLoad && <PageLoading />}
      <div className="shell">
      <aside>
        <div className="side-actions">
          {workspace === "ops" ? (
            <>
              <button className="primary" onClick={() => openNewSession(false)}>
                新建会话
              </button>
              <button
                type="button"
                onClick={() => {
                  setWorkspace("cluster");
                  setInspectedId(null);
                }}
              >
                项目集群
              </button>
            </>
          ) : (
            <>
              <button
                className="primary"
                onClick={() => {
                  setProjectName("");
                  setProjectDlg(true);
                }}
              >
                新建项目
              </button>
              <button
                type="button"
                onClick={() => {
                  setWorkspace("ops");
                  setSelectedProject(null);
                }}
              >
                常规运维
              </button>
            </>
          )}
        </div>
        <div className="nav-label">{workspace === "ops" ? "会话" : "集群"}</div>
        <ul
          className={`sess ${dropId === "root" ? "drop-root" : ""}`}
          data-drop="root"
          onContextMenu={(e) => {
            if ((e.target as HTMLElement).closest("li")) return;
            e.preventDefault();
            setCtx(null);
            setBlankCtx({ x: e.clientX, y: e.clientY });
          }}
        >
          {folderChildren(kindFolders, null).map(renderFolder)}
          {workspace === "ops"
            ? ungrouped.map(renderSession)
            : projects.filter((p) => !p.folder_id).map(renderProject)}
        </ul>
        <div className="sess-info">
          {inspected && (
            <dl>
              <div className="sess-info-row">
                <dt>名称</dt>
                <dd>
                  <span className="sess-info-val">{inspected.name || inspected.host}</span>
                </dd>
              </div>
              <div className="sess-info-row">
                <dt>IP</dt>
                <dd>
                  <CopyableValue text={inspected.host} />
                </dd>
              </div>
              <div className="sess-info-row">
                <dt>用户名</dt>
                <dd>
                  <CopyableValue text={inspected.username} />
                </dd>
              </div>
              <div className="sess-info-row">
                <dt>端口</dt>
                <dd>
                  <span className="sess-info-val">{String(inspected.port || "—")}</span>
                </dd>
              </div>
              <div className="sess-info-row">
                <dt>描述</dt>
                <dd>
                  <EllipsisHint text={inspected.notes.trim()} />
                </dd>
              </div>
            </dl>
          )}
        </div>
      </aside>
      <main>
        <div className="tabs">
          <div className="tab-strip" ref={tabStripRef}>
            {tabs.map((t) => (
              <button
                key={t.id}
                data-tab-id={t.id}
                className={t.id === currentId ? "on" : ""}
                onClick={() => {
                  setClusterOverview(false);
                  if (t.id === currentId) return;
                  setActive(t.id);
                }}
              >
                {liveTabLabel(t, tabs, sessions)}
                <span
                  className="x"
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTabs([t.id]);
                  }}
                >
                  ×
                </span>
              </button>
            ))}
          </div>
          {tab && (
            <div className="tab-actions">
              <button
                className={side === "term" ? "on" : ""}
                onClick={() => {
                  setSide("term");
                  setSideNonce((n) => n + 1);
                }}
              >
                终端
              </button>
              <button
                className={side === "sftp" ? "on" : ""}
                onClick={() => {
                  setSide("sftp");
                  setSideNonce((n) => n + 1);
                }}
              >
                SFTP
              </button>
              <button
                className={side === "monitor" ? "on" : ""}
                onClick={() => {
                  setSide("monitor");
                  setSideNonce((n) => n + 1);
                }}
              >
                监控
              </button>
            </div>
          )}
        </div>
        {err && <div className="banner">{err}</div>}
        {workspace === "cluster" && currentProject && clusterOverview && (
          <ClusterPane
            project={currentProject}
            nodes={currentProjectNodes}
            liveMap={liveMap}
            aiOn={aiOn}
            aiWidth={aiDockW}
            onAiWidth={setAiDockW}
            onConnect={(s) => void openSaved(s)}
            onAddFromOps={() => setPickNode(true)}
            onAddNew={() => openNewSession(true)}
          />
        )}
        {tabs.length === 0 && !(workspace === "cluster" && currentProject) && (
          <div className="empty">
            {workspace === "cluster" ? "新建或选择一个项目，然后添加节点。" : "点会话上的连接图标或双击会话即可连接。"}
          </div>
        )}
        {tab && !clusterOverview && (
          <div className={`term-stage${aiOn ? " with-ai" : ""}`} hidden={side !== "term"}>
            <TerminalView
              sessionId={tab.id}
              frame={tab.frame}
              fontFamily={settings?.font_family || "JetBrains Mono, Cascadia Mono, Sarasa Mono SC, Noto Sans Mono CJK SC, Microsoft YaHei, monospace"}
              fontSize={settings?.font_size || 14}
              defaultFg={resolveEyeCareFg(settings?.term_fg)}
              visible={side === "term"}
              refreshNonce={sideNonce}
              onFrame={(frame) => setTabs((all) => all.map((t) => (t.id === tab.id ? { ...t, frame } : t)))}
            />
            {aiOn &&
              tabs.map((t) => (
                <AiPanel
                  key={t.id}
                  sessionId={t.id}
                  width={aiDockW}
                  onWidth={setAiDockW}
                  visible={t.id === tab.id}
                />
              ))}
          </div>
        )}
        {tab && !clusterOverview && (
          <div hidden={side !== "sftp"} className="sftp-stage">
            <SftpPane
              key={tab.savedId || tab.id}
              sessionId={tab.id}
              savedId={tab.savedId}
              visible={side === "sftp"}
              openNonce={sideNonce}
              initialLocalPath={tab.savedId ? sessions.find((s) => s.id === tab.savedId)?.sftp_local_path : ""}
              onLocalPathChange={(path) => {
                const sid = tab.savedId;
                if (!sid) return;
                const s = sessions.find((x) => x.id === sid);
                if (!s || s.sftp_local_path === path) return;
                const next = { ...s, sftp_local_path: path };
                setSessions((all) => all.map((x) => (x.id === sid ? next : x)));
                void api.upsertSession(next);
              }}
              jobs={jobs.filter((j) => !j.session_id || j.session_id === tab.id)}
            />
          </div>
        )}
        {tab && !clusterOverview && side === "monitor" && <MonitorPane key={`${tab.id}-${sideNonce}`} sessionId={tab.id} />}
        {tab?.closed && <div className="banner">{tab.closed}</div>}
      </main>
      </div>
      {keepAbout && (
        <div className="page-overlay" hidden={!showAbout}>
          <AboutPage onClose={closeAbout} />
        </div>
      )}
      {keepSet && settings && (
        <div className="page-overlay" hidden={!showSet}>
          <SettingsPage
            settings={settings}
            onChange={setSettings}
            onImported={loadApp}
            onClose={() => setShowSet(false)}
          />
        </div>
      )}
      {keepAi && (
        <div className="page-overlay" hidden={!showAi}>
          <AiSettingsPage onClose={() => setShowAi(false)} onSaved={setAiOn} />
        </div>
      )}
      {keepNew && (
        <div className="page-overlay" hidden={!showNew}>
          <SessionForm
            title={editingId ? "会话属性" : newForCluster ? "新增节点" : "新建会话"}
            draft={draft}
            onChange={setDraft}
            folders={kindFolders}
            keys={keys}
            sessions={opsSessions}
            editing={!!editingId}
            showSyncOps={newForCluster}
            err={err}
            onSubmitConnect={() => void saveDraft(!editingId && !newForCluster)}
            onSaveOnly={() => void saveDraft(false)}
            onQuick={editingId ? undefined : quickConnect}
            onClose={() => {
              setShowNew(false);
              setEditingId(null);
              setNewForCluster(false);
            }}
          />
        </div>
      )}
      </div>

      {folderDlg && (
        <div className="modal" onClick={() => setFolderDlg(null)}>
          <form className="card" onClick={(e) => e.stopPropagation()} onSubmit={(e) => void submitFolder(e)}>
            <h2>{folderDlg.parentId ? "新建子分组" : "新建分组"}</h2>
            <label>
              分组名称
              <input
                value={folderName}
                onChange={(e) => setFolderName(e.target.value)}
                placeholder="例如：生产环境"
                autoFocus
              />
            </label>
            <div className="row card-actions">
              <button type="submit" className="primary" disabled={!folderName.trim()}>
                创建
              </button>
              <button type="button" onClick={() => setFolderDlg(null)}>
                取消
              </button>
            </div>
          </form>
        </div>
      )}

      {ctx && (
        <ContextMenu x={ctx.x} y={ctx.y}>
          <button
            type="button"
            onClick={() => {
              const s = sessions.find((x) => x.id === ctx.sessionId);
              setCtx(null);
              if (s) toggleSessionConn(s);
            }}
          >
            {liveIds.has(ctx.sessionId) ? "断开连接" : "连接"}
          </button>
          <button
            type="button"
            onClick={() => {
              const s = sessions.find((x) => x.id === ctx.sessionId);
              if (s) openEditSession(s);
            }}
          >
            属性
          </button>
          {workspace === "ops" && (
            <>
              <button type="button" onClick={() => moveSession(ctx.sessionId, null)}>
                移到未分组
              </button>
              {walkFolders(kindFolders).map(({ folder: f }) => (
                <button key={f.id} type="button" onClick={() => moveSession(ctx.sessionId, f.id)}>
                  移到 {folderPath(kindFolders, f.id)}
                </button>
              ))}
            </>
          )}
        </ContextMenu>
      )}

      {blankCtx && (
        <ContextMenu x={blankCtx.x} y={blankCtx.y}>
          <button
            type="button"
            onClick={() => {
              setBlankCtx(null);
              addFolder();
            }}
          >
            新增分组
          </button>
        </ContextMenu>
      )}

      {projectDlg && (
        <div className="modal" onClick={() => setProjectDlg(false)}>
          <form
            className="card"
            onClick={(e) => e.stopPropagation()}
            onSubmit={async (e) => {
              e.preventDefault();
              const name = projectName.trim();
              if (!name) return;
              await api.upsertProject({
                id: crypto.randomUUID(),
                folder_id: null,
                name,
                notes: "",
                sort_order: projects.length,
                updated_at: 0,
              });
              setProjects(await api.listProjects());
              setProjectDlg(false);
              setProjectName("");
            }}
          >
            <h2>新建项目</h2>
            <label>
              项目名称
              <input value={projectName} onChange={(e) => setProjectName(e.target.value)} autoFocus />
            </label>
            <div className="row card-actions">
              <button type="submit" className="primary" disabled={!projectName.trim()}>
                创建
              </button>
              <button type="button" onClick={() => setProjectDlg(false)}>
                取消
              </button>
            </div>
          </form>
        </div>
      )}

      {pickNode && (
        <div className="modal" onClick={() => setPickNode(false)}>
          <div className="card wide" onClick={(e) => e.stopPropagation()}>
            <h2>从常规运维选择会话</h2>
            <div className="pick-list">
              {opsSessions
                .filter((s) => !projectNodes.some((n) => n.project_id === selectedProject && n.session_id === s.id))
                .map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => {
                      if (!selectedProject) return;
                      const n = projectNodes.filter((x) => x.project_id === selectedProject).length;
                      void api.addProjectNode(selectedProject, s.id, n).then(async () => {
                        setProjectNodes(await api.listProjectNodes());
                        setPickNode(false);
                      });
                    }}
                  >
                    {s.name || s.host} · {s.username}@{s.host}
                  </button>
                ))}
            </div>
            <div className="row card-actions">
              <button type="button" onClick={() => setPickNode(false)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      )}

      {danger && (
        <div className="modal danger-modal">
          <div className="card wide" onClick={(e) => e.stopPropagation()}>
            <h2>AI 申请执行危险命令</h2>
            <p className="warn">该命令匹配危险策略，必须由你同意后才会发送到服务器。可先修正再同意，或拒绝让 AI 另寻方案。</p>
            <label>
              命令内容
              <textarea value={dangerCmd} onChange={(e) => setDangerCmd(e.target.value)} rows={4} />
            </label>
            <div className="row card-actions">
              <button
                type="button"
                className="primary"
                onClick={() => {
                  void api.aiDangerReply(danger.sessionId, true, dangerCmd);
                  setDanger(null);
                }}
              >
                同意
              </button>
              <button
                type="button"
                onClick={() => {
                  void api.aiDangerReply(danger.sessionId, false, dangerCmd);
                  setDanger(null);
                }}
              >
                拒绝
              </button>
            </div>
          </div>
        </div>
      )}

      {hostPrompt && (
        <div className="modal" onClick={() => setHostPrompt(null)}>
          <div className="card wide" onClick={(e) => e.stopPropagation()}>
            <h2>{hostPrompt.kind === "mismatch" ? "主机密钥已变更" : "未知主机密钥"}</h2>
            <p className={hostPrompt.kind === "mismatch" ? "warn" : "hint"}>
              {hostPrompt.kind === "mismatch"
                ? "该主机的指纹与保险库中记录的不一致。可能是服务器重装，也可能是中间人攻击。请核对后再决定是否替换。"
                : "首次连接该主机。请向管理员核对指纹，确认无误后再信任。"}
            </p>
            <label>
              主机
              <div className="fp">
                {hostPrompt.host}:{hostPrompt.port}
              </div>
            </label>
            {hostPrompt.expected && (
              <label>
                已记录指纹
                <div className="fp muted-fp">{hostPrompt.expected}</div>
              </label>
            )}
            <label>
              {hostPrompt.kind === "mismatch" ? "当前指纹" : "SHA256 指纹"}
              <div className="fp">{hostPrompt.fingerprint || "（握手未返回指纹，将在信任后写入）"}</div>
            </label>
            <div className="row">
              <button
                className="primary"
                type="button"
                onClick={async () => {
                  await hostPrompt.retry();
                }}
              >
                {hostPrompt.kind === "mismatch" ? "替换并连接" : "信任并连接"}
              </button>
              <button type="button" onClick={() => setHostPrompt(null)}>
                取消
              </button>
            </div>
          </div>
        </div>
      )}
      {ghost && (
        <div className="tree-ghost" style={{ left: ghost.x + 12, top: ghost.y + 12 }}>
          移动 {ghost.label}
        </div>
      )}
      {chromeDialogs}
      <ToastHost />
    </div>
  );
}
