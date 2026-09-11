import { useEffect, useMemo, useRef, useState, type Ref } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type PathStat } from "./api";
import type { RemoteEntry, TransferProgress } from "./types";
import PageLoading from "./Loading";
import ConflictDialog, { type ConflictKind } from "./ConflictDialog";
import ContextMenu from "./ContextMenu";

type Props = {
  sessionId: string;
  savedId?: string;
  initialLocalPath?: string;
  onLocalPathChange?: (path: string) => void;
  jobs: TransferProgress[];
  visible?: boolean;
  openNonce?: number;
};

function fmtSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function xferMark(direction: string) {
  if (direction === "upload") return "↑";
  if (direction === "delete") return "×";
  return "↓";
}

function xferAmount(j: TransferProgress) {
  if (j.direction === "delete") return `${j.transferred} / ${j.total} 项`;
  return `${fmtSize(j.transferred)} / ${fmtSize(j.total)}`;
}

function parentPath(p: string) {
  const win = p.includes("\\");
  const norm = p.replace(/\\/g, "/").replace(/\/+$/, "");
  const i = norm.lastIndexOf("/");
  if (i <= 0) return win ? "" : "/";
  const next = norm.slice(0, i);
  return win ? next.replace(/\//g, "\\") : next || "/";
}

function joinDest(dir: string, name: string) {
  const base = (dir || ".").replace(/[\\/]+$/, "");
  const sep = dir.includes("\\") ? "\\" : "/";
  return `${base}${sep}${name}`;
}

function isNoiseStatus(raw: string) {
  return /no such file/i.test(raw);
}

function showErr(e: unknown) {
  const s = String(e).replace(/^Error:\s*/i, "").trim();
  if (isNoiseStatus(s)) return "";
  return s;
}

function formatXferErr(e: unknown) {
  const s = String(e).replace(/^Error:\s*/i, "").trim();
  if (!s) return "传输失败";
  const lower = s.toLowerCase();
  if (/permission denied|permissiondenied/.test(lower) && !s.includes("没有权限")) {
    return `没有权限：${s}`;
  }
  return s;
}

function timed<T>(p: Promise<T>, ms: number, fallback: T): Promise<T> {
  return new Promise((resolve) => {
    const t = window.setTimeout(() => resolve(fallback), ms);
    p.then(
      (v) => {
        window.clearTimeout(t);
        resolve(v);
      },
      () => {
        window.clearTimeout(t);
        resolve(fallback);
      },
    );
  });
}

function sftpStartPath(cwd: string) {
  const t = cwd.trim();
  if (!t || t === ".") return "/";
  return t;
}

function clearDragSelection() {
  const sel = window.getSelection();
  if (sel && sel.rangeCount) sel.removeAllRanges();
}

function pointIn(el: HTMLElement | null, x: number, y: number) {
  if (!el) return false;
  const r = el.getBoundingClientRect();
  return x >= r.left - 2 && x <= r.right + 2 && y >= r.top - 2 && y <= r.bottom + 2;
}

function fmtMtime(n: number) {
  if (!n) return "";
  const d = new Date(n * 1000);
  if (Number.isNaN(d.getTime())) return "";
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function normalizeRemotePath(raw: string) {
  const t = raw.trim().replace(/\\/g, "/");
  if (!t || t === "~" || t === "~/") return "/";
  const collapsed = t.replace(/\/{2,}/g, "/");
  return collapsed.length > 1 ? collapsed.replace(/\/+$/, "") : collapsed || "/";
}

function ArrowUp() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <path d="M8 2.2 13.2 8h-3.1v5.8H5.9V8H2.8L8 2.2z" />
    </svg>
  );
}

function ArrowDown() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <path d="M8 13.8 2.8 8h3.1V2.2h4.2V8h3.1L8 13.8z" />
    </svg>
  );
}

export default function SftpPane({
  sessionId,
  initialLocalPath,
  onLocalPathChange,
  jobs,
  visible = true,
  openNonce = 0,
}: Props) {
  const [localPath, setLocalPath] = useState(initialLocalPath || "");
  const [remotePath, setRemotePath] = useState("");
  const [local, setLocal] = useState<RemoteEntry[]>([]);
  const [remote, setRemote] = useState<RemoteEntry[]>([]);
  const [checkedL, setCheckedL] = useState<string[]>([]);
  const [checkedR, setCheckedR] = useState<string[]>([]);
  const [status, setStatus] = useState("");
  const [statusErr, setStatusErr] = useState(false);
  const [dockOpen, setDockOpen] = useState(false);
  const [dockTab, setDockTab] = useState<"active" | "done">("active");
  const [tip, setTip] = useState<{ text: string; x: number; y: number } | null>(null);
  const [booting, setBooting] = useState(true);
  const [conflict, setConflict] = useState<{ entry: RemoteEntry; dest: string; remaining: number } | null>(null);
  const [dropOver, setDropOver] = useState(false);
  const conflictWait = useRef<((k: ConflictKind) => void) | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const localPaneRef = useRef<HTMLDivElement>(null);
  const remotePaneRef = useRef<HTMLDivElement>(null);
  const localPathRef = useRef(localPath);
  const remotePathRef = useRef(remotePath);
  const visibleRef = useRef(visible);
  const dropGuard = useRef(0);
  const scaleRef = useRef(1);
  const dropOverRef = useRef(false);
  const localGen = useRef(0);
  const remoteGen = useRef(0);
  const bootGen = useRef(0);
  const takeDroppedRef = useRef<(paths: string[]) => void>(() => {});
  const internalDrag = useRef<{ paths: string[]; x: number; y: number; active: boolean } | null>(null);
  localPathRef.current = localPath;
  remotePathRef.current = remotePath;
  visibleRef.current = visible;

  function note(text: string, err = false) {
    setStatus(text);
    setStatusErr(err);
  }

  function clearNote() {
    setStatus("");
    setStatusErr(false);
  }

  function setRemoteDropOver(on: boolean) {
    dropOverRef.current = on;
    setDropOver(on);
  }

  function setLocalAndKeep(next: string) {
    setLocalPath(next);
    if (next) onLocalPathChange?.(next);
  }

  function askConflict(entry: RemoteEntry, dest: string, remaining: number) {
    return new Promise<ConflictKind>((resolve) => {
      conflictWait.current = resolve;
      setConflict({ entry, dest, remaining });
    });
  }

  function pickConflict(kind: ConflictKind) {
    const wait = conflictWait.current;
    conflictWait.current = null;
    setConflict(null);
    wait?.(kind);
  }

  async function refreshLocal(path = localPathRef.current) {
    const gen = ++localGen.current;
    const listPath = path;
    try {
      const entries = await api.listLocal(listPath);
      if (gen !== localGen.current) return;
      if (listPath !== localPathRef.current) return;
      setLocal(entries);
      setBooting(false);
      if (!localPathRef.current && entries[0]) {
        setLocalAndKeep(parentPath(entries[0].path));
      }
    } catch (e) {
      setBooting(false);
      const msg = showErr(e);
      if (msg) note(msg, true);
    }
  }

  async function refreshRemote(path?: string) {
    const gen = ++remoteGen.current;
    const listPath = normalizeRemotePath(path || remotePathRef.current || "/");
    try {
      const entries = await api.sftpList(sessionId, listPath);
      if (gen !== remoteGen.current) return;
      if (normalizeRemotePath(remotePathRef.current || "/") !== listPath) return;
      setRemote(entries);
      setBooting(false);
    } catch (e) {
      setBooting(false);
      const msg = showErr(e);
      if (msg) note(msg, true);
    }
  }

  async function refresh(path?: string) {
    const remote = normalizeRemotePath(path || remotePathRef.current || "/");
    await Promise.all([refreshLocal(localPathRef.current), refreshRemote(remote)]);
  }

  useEffect(() => {
    if (!visible) {
      setBooting(false);
      return;
    }
    const id = ++bootGen.current;
    const empty = local.length === 0 && remote.length === 0;
    setBooting(empty);
    let cancelled = false;
    const safety = window.setTimeout(() => {
      if (id === bootGen.current) setBooting(false);
    }, 8000);
    (async () => {
      try {
        const raw = await timed(api.termCwd(sessionId), 4000, "");
        const path = normalizeRemotePath(sftpStartPath(raw));
        if (cancelled || id !== bootGen.current) return;
        if (path !== remotePathRef.current) setRemotePath(path);
        await refresh(path);
        if (!cancelled) {
          setCheckedL([]);
          setCheckedR([]);
        }
      } finally {
        window.clearTimeout(safety);
        if (!cancelled && id === bootGen.current) setBooting(false);
      }
    })();
    return () => {
      cancelled = true;
      window.clearTimeout(safety);
      if (id === bootGen.current) setBooting(false);
    };
  }, [sessionId, visible, openNonce]);

  useEffect(() => {
    void refreshLocal(localPath);
    setCheckedL([]);
  }, [localPath, sessionId]);

  useEffect(() => {
    if (!remotePath) return;
    void refreshRemote(remotePath);
    setCheckedR([]);
  }, [remotePath, sessionId]);

  useEffect(() => {
    if (!status || statusErr || /…$/.test(status)) return;
    const id = window.setTimeout(() => clearNote(), 10_000);
    return () => window.clearTimeout(id);
  }, [status, statusErr]);

  useEffect(() => {
    let un: (() => void) | undefined;
    const win = getCurrentWindow();
    function candidates(pos: { x: number; y: number }) {
      const factor = scaleRef.current || 1;
      const dpr = window.devicePixelRatio || 1;
      return [
        { x: pos.x / factor, y: pos.y / factor },
        { x: pos.x / dpr, y: pos.y / dpr },
        { x: pos.x, y: pos.y },
      ];
    }
    function hits(el: HTMLElement | null, pos: { x: number; y: number }) {
      return candidates(pos).some((p) => pointIn(el, p.x, p.y));
    }
    function overRemote(pos: { x: number; y: number }) {
      if (hits(remotePaneRef.current, pos)) return true;
      if (hits(localPaneRef.current, pos)) return false;
      const panes = rootRef.current?.querySelector(".sftp-panes");
      if (!(panes instanceof HTMLElement)) return false;
      const r = panes.getBoundingClientRect();
      return candidates(pos).some((p) => p.x >= r.left + r.width / 2 && p.x <= r.right && p.y >= r.top && p.y <= r.bottom);
    }
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (!visibleRef.current) {
          document.body.classList.remove("sftp-copy-drag");
          setRemoteDropOver(false);
          return;
        }
        const payload = e.payload;
        if (payload.type === "leave") {
          document.body.classList.remove("sftp-copy-drag");
          setRemoteDropOver(false);
          return;
        }
        const pos = payload.position;
        if (payload.type === "drop") {
          document.body.classList.remove("sftp-copy-drag");
          const accept = dropOverRef.current || overRemote(pos);
          setRemoteDropOver(false);
          if (!accept || !payload.paths.length) {
            void (async () => {
              try {
                scaleRef.current = await win.scaleFactor();
              } catch {
                /* keep last */
              }
              if (overRemote(pos) && payload.paths.length) takeDroppedRef.current(payload.paths);
            })();
            return;
          }
          takeDroppedRef.current(payload.paths);
          return;
        }
        void (async () => {
          if (payload.type === "enter") {
            try {
              scaleRef.current = await win.scaleFactor();
            } catch {
              /* keep last */
            }
          }
          setRemoteDropOver(overRemote(pos));
          document.body.classList.add("sftp-copy-drag");
          clearDragSelection();
        })();
      })
      .then((f) => {
        un = f;
      });
    return () => un?.();
  }, [sessionId]);

  useEffect(() => {
    function move(e: PointerEvent) {
      const d = internalDrag.current;
      if (!d) return;
      if (!d.active) {
        const dx = e.clientX - d.x;
        const dy = e.clientY - d.y;
        if (dx * dx + dy * dy < 36) return;
        d.active = true;
        document.body.classList.add("sftp-copy-drag");
        clearDragSelection();
      }
      e.preventDefault();
      clearDragSelection();
      setRemoteDropOver(pointIn(remotePaneRef.current, e.clientX, e.clientY));
    }
    function up(e: PointerEvent) {
      const d = internalDrag.current;
      internalDrag.current = null;
      document.body.classList.remove("sftp-copy-drag");
      clearDragSelection();
      if (!d?.active) {
        setRemoteDropOver(false);
        return;
      }
      const over = pointIn(remotePaneRef.current, e.clientX, e.clientY);
      setRemoteDropOver(false);
      if (over && d.paths.length) takeDroppedRef.current(d.paths);
    }
    function blockSelect(e: Event) {
      if (internalDrag.current || dropOverRef.current || document.body.classList.contains("sftp-copy-drag")) {
        e.preventDefault();
        clearDragSelection();
      }
    }
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    document.addEventListener("selectstart", blockSelect);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      document.removeEventListener("selectstart", blockSelect);
      document.body.classList.remove("sftp-copy-drag");
    };
  }, [sessionId]);

  const active = useMemo(() => jobs.filter((j) => !j.finished), [jobs]);
  const done = useMemo(() => jobs.filter((j) => j.finished), [jobs]);
  const current = active[active.length - 1] ?? null;
  const showDock = active.length > 0 || (dockOpen && done.length > 0);

  function pick(list: RemoteEntry[], paths: string[]) {
    return list.filter((e) => paths.includes(e.path));
  }

  async function rejectEmpty(localSide: boolean, entries: RemoteEntry[]) {
    const empties: string[] = [];
    for (const e of entries) {
      if (!e.is_dir) continue;
      const kids = localSide ? await api.listLocal(e.path) : await api.sftpList(sessionId, e.path);
      if (kids.length === 0) empties.push(e.name);
    }
    if (empties.length) {
      throw new Error(`空文件夹不能${localSide ? "上传" : "下载"}：${empties.join("、")}`);
    }
  }

  async function confirmUploaded(dest: string, item: { path: string; name: string; is_dir: boolean }) {
    const local = await api.localStat(item.path);
    const st = await api.sftpStat(sessionId, dest);
    if (!st.exists) throw new Error(`远端没有 ${item.name}`);
    if (!item.is_dir && st.size !== local.size) {
      throw new Error(`远端大小 ${st.size}，本地 ${local.size}`);
    }
    const now = Math.floor(Date.now() / 1000);
    if (!item.is_dir && st.mtime + 2 < now) {
      return { ...st, mtime: now };
    }
    return st;
  }

  function patchRemoteMeta(st: PathStat) {
    setRemote((list) => {
      const next = list.map((e) =>
        e.path === st.path ? { ...e, size: st.size, mtime: st.mtime, is_dir: st.is_dir } : e,
      );
      if (next.some((e) => e.path === st.path)) return next;
      return [
        ...next,
        {
          name: st.name,
          path: st.path,
          is_dir: st.is_dir,
          is_symlink: false,
          size: st.size,
          mode: 0,
          mtime: st.mtime,
          longname: st.name,
        },
      ];
    });
  }

  async function uploadItems(items: { path: string; name: string; is_dir: boolean }[]) {
    if (!items.length) return;
    note("上传中…");
    try {
      await rejectEmpty(
        true,
        items.map((i) => ({ ...emptyEntry(i.path, i.name, i.is_dir) })),
      );
      let policy: ConflictKind | null = null;
      const pending = [...items];
      const uploaded: PathStat[] = [];
      for (let i = 0; i < pending.length; i++) {
        const item = pending[i];
        const dest = joinDest(remotePathRef.current || remotePath || "/", item.name);
        const st = await api.sftpStat(sessionId, dest);
        let overwrite = false;
        if (st.exists) {
          const kind: ConflictKind =
            policy === "overwrite_all" || policy === "skip_all"
              ? policy
              : await askConflict(emptyEntry(item.path, item.name, item.is_dir || st.is_dir), dest, pending.length - i);
          if (kind === "cancel") {
            note("已取消上传");
            return;
          }
          if (kind === "overwrite_all" || kind === "skip_all") policy = kind;
          if (kind === "skip" || kind === "skip_all") continue;
          overwrite = kind === "overwrite" || kind === "overwrite_all";
        }
        try {
          await api.sftpTransfer(sessionId, item.path, dest, true, !overwrite, overwrite);
          uploaded.push(await confirmUploaded(dest, item));
        } catch (e) {
          note(`上传失败 ${item.name}：${formatXferErr(e)}`, true);
          await refreshRemote();
          return;
        }
      }
      setCheckedL([]);
      await refresh();
      for (const st of uploaded) patchRemoteMeta(st);
      if (!uploaded.length) note("没有上传（已跳过）");
      else if (uploaded.length === 1) note(`已上传 ${uploaded[0].name}`);
      else note(`已上传 ${uploaded.length} 项`);
    } catch (e) {
      const msg = formatXferErr(e);
      if (msg) note(msg, true);
    } finally {
      setConflict(null);
    }
  }

  function emptyEntry(path: string, name: string, is_dir: boolean): RemoteEntry {
    return { name, path, is_dir, is_symlink: false, size: 0, mode: 0, mtime: 0, longname: name };
  }

  async function upload() {
    const items = pick(local, checkedL);
    await uploadItems(items);
  }

  async function uploadPaths(paths: string[]) {
    if (!paths.length) return;
    const items: { path: string; name: string; is_dir: boolean }[] = [];
    for (const p of paths) {
      const st = await api.localStat(p);
      items.push({ path: p, name: st.name || p.replace(/^.*[\\/]/, ""), is_dir: st.is_dir });
    }
    await uploadItems(items);
  }

  function takeDroppedPaths(paths: string[]) {
    const unique = [...new Set(paths.filter(Boolean))];
    if (!unique.length) return;
    const now = Date.now();
    if (now - dropGuard.current < 400) return;
    dropGuard.current = now;
    void uploadPaths(unique);
  }
  takeDroppedRef.current = takeDroppedPaths;

  async function download() {
    const items = pick(remote, checkedR);
    if (!items.length) return;
    note("下载中…");
    try {
      await rejectEmpty(false, items);
      for (const item of items) {
        const dest = joinDest(localPathRef.current || localPath || ".", item.name);
        await api.sftpTransfer(sessionId, dest, item.path, false, true, false);
      }
      note("下载完成");
      setCheckedR([]);
      await refresh();
    } catch (e) {
      note(`下载失败：${formatXferErr(e)}`, true);
    }
  }

  async function deleteEntries(localSide: boolean, entries: RemoteEntry[]) {
    if (!entries.length) return;
    const label = entries.length === 1 ? entries[0].name : `选中的 ${entries.length} 项`;
    if (!confirm(`删除 ${label}？`)) return;
    note("删除中…");
    try {
      for (const e of entries) {
        if (localSide) await api.removeLocal(e.path, sessionId);
        else await api.sftpRemove(sessionId, e.path, e.is_dir);
      }
      if (localSide) {
        setCheckedL((cur) => cur.filter((p) => !entries.some((e) => e.path === p)));
        await refreshLocal();
      } else {
        setCheckedR((cur) => cur.filter((p) => !entries.some((e) => e.path === p)));
        await refreshRemote();
      }
      note(entries.length === 1 ? `已删除 ${entries[0].name}` : `已删除 ${entries.length} 项`);
    } catch (e) {
      note(`删除失败：${formatXferErr(e)}`, true);
      if (localSide) await refreshLocal();
      else await refreshRemote();
    }
  }

  function goRemote(raw: string) {
    const next = normalizeRemotePath(raw);
    if (next === remotePath) void refreshRemote(next);
    else setRemotePath(next);
  }

  const dockJobs = dockTab === "active" ? active : done;

  return (
    <div ref={rootRef} className={`sftp${showDock ? " has-dock" : ""}${dockOpen ? " xfer-open" : ""}`}>
      {booting && <PageLoading text="正在加载文件列表…" />}
      <div className="sftp-toolbar">
        <button type="button" className="xfer-btn" onClick={upload} disabled={!checkedL.length}>
          <ArrowUp />
          上传
        </button>
        <button type="button" className="xfer-btn" onClick={download} disabled={!checkedR.length}>
          <ArrowDown />
          下载
        </button>
        {status && (
          <span className={`sftp-note${statusErr ? " err" : ""}`}>
            <span className="sftp-note-text">{status}</span>
            <button type="button" className="sftp-note-x" title="关闭提示" onClick={clearNote}>
              ×
            </button>
          </span>
        )}
      </div>
      <div className="sftp-panes">
        <Pane
          title="本地"
          path={localPath}
          setPath={setLocalAndKeep}
          entries={local}
          checked={checkedL}
          setChecked={setCheckedL}
          pickFolder
          dragLocal
          paneRef={localPaneRef}
          onInternalDragStart={(paths, x, y) => {
            internalDrag.current = { paths, x, y, active: false };
          }}
          onRefresh={() => void refreshLocal()}
          onMkdir={async (name) => {
            await api.mkdirLocal(joinDest(localPath || ".", name));
            await refreshLocal();
          }}
          onCreateFile={async (name) => {
            await api.createLocalFile(joinDest(localPath || ".", name));
            await refreshLocal();
          }}
          onDelete={(entries) => void deleteEntries(true, entries)}
          onTip={setTip}
        />
        <Pane
          title="远端"
          path={remotePath}
          setPath={goRemote}
          entries={remote}
          checked={checkedR}
          setChecked={setCheckedR}
          dropUpload
          dropOver={dropOver}
          paneRef={remotePaneRef}
          onHover={setRemoteDropOver}
          onDropPaths={takeDroppedPaths}
          onRefresh={() => void refreshRemote()}
          onMkdir={async (name) => {
            await api.sftpMkdir(sessionId, joinDest(remotePath || "/", name));
            await refreshRemote();
          }}
          onCreateFile={async (name) => {
            await api.sftpCreateFile(sessionId, joinDest(remotePath || "/", name));
            await refreshRemote();
          }}
          onDelete={(entries) => void deleteEntries(false, entries)}
          onTip={setTip}
        />
      </div>
      {showDock && (
        <div className={`xfer-dock${dockOpen ? " open" : ""}`}>
          {!dockOpen && current && (
            <button type="button" className="xfer-one" onClick={() => setDockOpen(true)}>
              <span>
                {xferMark(current.direction)} {current.path}
              </span>
              <progress value={current.transferred} max={Math.max(1, current.total)} />
              <span>{xferAmount(current)}</span>
              <span className="muted">展开</span>
            </button>
          )}
          {!dockOpen && !current && done.length > 0 && (
            <button type="button" className="xfer-one" onClick={() => setDockOpen(true)}>
              <span>已完成 {done.length} 项</span>
              <span className="muted">展开</span>
            </button>
          )}
          {dockOpen && (
            <>
              <div className="xfer-tabs">
                <button type="button" className={dockTab === "active" ? "on" : ""} onClick={() => setDockTab("active")}>
                  进行中 {active.length}
                </button>
                <button type="button" className={dockTab === "done" ? "on" : ""} onClick={() => setDockTab("done")}>
                  已完成 {done.length}
                </button>
                <span className="spacer" />
                <button type="button" onClick={() => setDockOpen(false)}>
                  收起
                </button>
              </div>
              <div className="xfer-list">
                {dockJobs.length === 0 && <p className="muted">{dockTab === "active" ? "当前没有正在进行的任务。" : "还没有完成的任务。"}</p>}
                {dockJobs.map((j) => (
                  <div key={j.job_id + j.path} className="job">
                    <span>
                      {xferMark(j.direction)} {j.path}
                    </span>
                    <progress value={j.transferred} max={Math.max(1, j.total)} />
                    <span>
                      {xferAmount(j)}
                      {j.error ? ` · ${j.error}` : ""}
                    </span>
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
      )}
      {tip && (
        <div className="name-tip" style={{ left: tip.x, top: tip.y }}>
          {tip.text}
        </div>
      )}
      {conflict && (
        <ConflictDialog
          entry={conflict.entry}
          dest={conflict.dest}
          remaining={conflict.remaining}
          onPick={pickConflict}
        />
      )}
    </div>
  );
}

function Pane(props: {
  title: string;
  path: string;
  setPath: (p: string) => void;
  entries: RemoteEntry[];
  checked: string[];
  setChecked: (paths: string[]) => void;
  pickFolder?: boolean;
  dragLocal?: boolean;
  dropUpload?: boolean;
  dropOver?: boolean;
  paneRef?: Ref<HTMLDivElement>;
  onHover?: (on: boolean) => void;
  onDropPaths?: (paths: string[]) => void;
  onInternalDragStart?: (paths: string[], x: number, y: number) => void;
  onRefresh?: () => void;
  onMkdir?: (name: string) => void | Promise<void>;
  onCreateFile?: (name: string) => void | Promise<void>;
  onDelete?: (entries: RemoteEntry[]) => void;
  onTip?: (tip: { text: string; x: number; y: number } | null) => void;
}) {
  const allPaths = props.entries.map((e) => e.path);
  const allOn = allPaths.length > 0 && allPaths.every((p) => props.checked.includes(p));
  const [menu, setMenu] = useState<{ x: number; y: number; entry?: RemoteEntry } | null>(null);
  const [pathDraft, setPathDraft] = useState(props.path);

  useEffect(() => {
    setPathDraft(props.path);
  }, [props.path]);

  function toggle(path: string, on: boolean) {
    props.setChecked(on ? [...props.checked.filter((p) => p !== path), path] : props.checked.filter((p) => p !== path));
  }

  async function pickLocalFolder() {
    const dir = await open({
      directory: true,
      multiple: false,
      defaultPath: props.path || undefined,
      title: "选择本地文件夹",
    });
    if (typeof dir === "string" && dir) props.setPath(dir);
  }

  function askName(kind: "folder" | "file") {
    const name = prompt(kind === "folder" ? "新文件夹名" : "新文件名");
    if (!name) return;
    if (kind === "folder") void props.onMkdir?.(name);
    else void props.onCreateFile?.(name);
    setMenu(null);
  }

  function showTip(el: HTMLElement, text: string) {
    if (el.scrollWidth <= el.clientWidth + 1) {
      props.onTip?.(null);
      return;
    }
    const r = el.getBoundingClientRect();
    props.onTip?.({ text, x: r.left, y: r.bottom + 6 });
  }

  function deleteTargets(clicked?: RemoteEntry) {
    const selected = props.entries.filter((e) => props.checked.includes(e.path));
    if (clicked && !selected.some((e) => e.path === clicked.path)) return [clicked];
    if (selected.length) return selected;
    return clicked ? [clicked] : [];
  }

  return (
    <div
      ref={props.paneRef}
      className={`pane${props.dropOver ? " drop-over" : ""}`}
      onClick={() => setMenu(null)}
      onDragEnter={(e) => {
        if (!props.dropUpload) return;
        e.preventDefault();
        props.onHover?.(true);
      }}
      onDragLeave={(e) => {
        if (!props.dropUpload) return;
        if (e.currentTarget.contains(e.relatedTarget as Node)) return;
        props.onHover?.(false);
      }}
      onDragOver={(e) => {
        if (!props.dropUpload) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
      }}
      onDrop={(e) => {
        if (!props.dropUpload) return;
        e.preventDefault();
        props.onHover?.(false);
        const custom = e.dataTransfer.getData("application/x-ferrassh-local");
        const paths: string[] = [];
        if (custom) {
          try {
            paths.push(...(JSON.parse(custom) as string[]));
          } catch {
            /* ignore */
          }
        }
        for (const f of Array.from(e.dataTransfer.files)) {
          const p = (f as File & { path?: string }).path;
          if (p) paths.push(p);
        }
        if (paths.length) props.onDropPaths?.(paths);
      }}
    >
      <div className="pane-title">{props.title}</div>
      <div className="list-head">
        <label className="check-mini" title="全选">
          <input
            type="checkbox"
            checked={allOn}
            onChange={(e) => props.setChecked(e.target.checked ? allPaths : [])}
          />
        </label>
        {props.pickFolder ? (
          <button type="button" className="path-btn" title={props.path || "/"} onClick={() => void pickLocalFolder()}>
            {props.path || "/"}
          </button>
        ) : (
          <input
            className="path-btn path-input"
            value={pathDraft}
            title={pathDraft || "/"}
            spellCheck={false}
            onChange={(e) => setPathDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                props.setPath(pathDraft);
              }
            }}
            onBlur={() => setPathDraft(props.path)}
          />
        )}
        <button
          type="button"
          className="pane-action"
          onClick={() => {
            const name = prompt("新文件夹名");
            if (name) void props.onMkdir?.(name);
          }}
        >
          新建
        </button>
        <button type="button" className="pane-action" onClick={() => props.onRefresh?.()}>
          刷新
        </button>
      </div>
      <ul
        onContextMenu={(ev) => {
          ev.preventDefault();
          setMenu({ x: ev.clientX, y: ev.clientY });
        }}
      >
        <li
          onDoubleClick={() => {
            props.setPath(parentPath(props.path));
          }}
        >
          <span className="check-mini" />
          <span className="icon">📁</span>
          <span className="name">..</span>
          <span className="size" />
          <span className="mtime" />
        </li>
        {props.entries.map((e) => {
          const on = props.checked.includes(e.path);
          return (
            <li
              key={e.path}
              className={on ? "sel" : ""}
              onPointerDown={(ev) => {
                if (!props.dragLocal || ev.button !== 0) return;
                if ((ev.target as HTMLElement).closest(".check-mini")) return;
                ev.preventDefault();
                clearDragSelection();
                const paths = props.checked.includes(e.path) && props.checked.length ? props.checked : [e.path];
                props.onInternalDragStart?.(paths, ev.clientX, ev.clientY);
              }}
              onDoubleClick={() => {
                if (e.is_dir) props.setPath(e.path);
              }}
              onContextMenu={(ev) => {
                ev.preventDefault();
                ev.stopPropagation();
                setMenu({ x: ev.clientX, y: ev.clientY, entry: e });
              }}
            >
              <label className="check-mini" onClick={(ev) => ev.stopPropagation()}>
                <input type="checkbox" checked={on} onChange={(ev) => toggle(e.path, ev.target.checked)} />
              </label>
              <span className="icon">{e.is_dir ? "📁" : e.is_symlink ? "🔗" : "📄"}</span>
              <span
                className="name"
                onMouseEnter={(ev) => showTip(ev.currentTarget, e.name)}
                onMouseLeave={() => props.onTip?.(null)}
              >
                {e.name}
              </span>
              <span className="size">{e.is_dir ? "" : fmtSize(e.size)}</span>
              <span className="mtime" title={fmtMtime(e.mtime)}>
                {fmtMtime(e.mtime)}
              </span>
            </li>
          );
        })}
      </ul>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y}>
          <button type="button" onClick={() => askName("folder")}>
            新建文件夹
          </button>
          <button type="button" onClick={() => askName("file")}>
            新建文件
          </button>
          {(() => {
            const targets = deleteTargets(menu.entry);
            if (!targets.length || !props.onDelete) return null;
            return (
              <button
                type="button"
                onClick={() => {
                  props.onDelete?.(targets);
                  setMenu(null);
                }}
              >
                删除{targets.length > 1 ? ` (${targets.length})` : ""}
              </button>
            );
          })()}
        </ContextMenu>
      )}
    </div>
  );
}
