import type { Folder, SavedKey, SavedSession } from "./types";
import { emptyAlgs } from "./types";

export type AuthKind = SavedSession["auth_method"];
export type Profile = "modern" | "compatible" | "legacy";

export type SessionDraft = {
  name: string;
  host: string;
  port: number;
  username: string;
  password: string;
  passphrase: string;
  auth_method: AuthKind;
  key_id: string;
  jump_host_id: string;
  notes: string;
  folder_id: string;
  profile: Profile;
  local_echo: boolean;
  in_ops: boolean;
};

export function emptySessionDraft(): SessionDraft {
  return {
    name: "",
    host: "",
    port: 22,
    username: "root",
    password: "",
    passphrase: "",
    auth_method: "password",
    key_id: "",
    jump_host_id: "",
    notes: "",
    folder_id: "",
    profile: "modern",
    local_echo: false,
    in_ops: true,
  };
}

export function draftFromSession(s: SavedSession): SessionDraft {
  const profile = s.algs.profile === "compatible" || s.algs.profile === "legacy" ? s.algs.profile : "modern";
  return {
    name: s.name,
    host: s.host,
    port: s.port,
    username: s.username,
    password: "",
    passphrase: "",
    auth_method: s.auth_method,
    key_id: s.key_id || "",
    jump_host_id: s.jump_host_id || "",
    notes: s.notes,
    folder_id: s.folder_id || "",
    profile,
    local_echo: s.local_echo,
    in_ops: s.in_ops !== false,
  };
}

export function sessionFromDraft(draft: SessionDraft, id: string, sortOrder: number, existing?: SavedSession): SavedSession {
  return {
    id,
    folder_id: draft.folder_id || null,
    name: draft.name.trim() || draft.host.trim() || "会话",
    host: draft.host.trim(),
    port: Number(draft.port) || 22,
    username: draft.username.trim() || "root",
    auth_method: draft.auth_method,
    key_id: draft.auth_method === "key" && draft.key_id ? draft.key_id : null,
    jump_host_id: draft.jump_host_id || null,
    algs: { ...emptyAlgs(), profile: draft.profile },
    local_echo: draft.local_echo,
    keepalive: existing?.keepalive ?? 30,
    compression: existing?.compression ?? false,
    term: existing?.term || "xterm-256color",
    notes: draft.notes,
    sort_order: existing?.sort_order ?? sortOrder,
    updated_at: existing?.updated_at ?? 0,
    has_secret: existing?.has_secret ?? true,
    in_ops: draft.in_ops,
    sftp_local_path: existing?.sftp_local_path ?? "",
  };
}

function walkFolders(folders: Folder[], parentId: string | null = null, depth = 0): { folder: Folder; depth: number }[] {
  return folders
    .filter((f) => (f.parent_id ?? null) === parentId)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name, "zh"))
    .flatMap((folder) => [{ folder, depth }, ...walkFolders(folders, folder.id, depth + 1)]);
}

type Props = {
  title: string;
  draft: SessionDraft;
  onChange: (next: SessionDraft) => void;
  folders: Folder[];
  keys: SavedKey[];
  sessions: SavedSession[];
  editing: boolean;
  showSyncOps?: boolean;
  err?: string;
  onSubmitConnect: () => void;
  onSaveOnly: () => void;
  onQuick?: () => void;
  onClose: () => void;
};

export default function SessionForm({
  title,
  draft,
  onChange,
  folders,
  keys,
  sessions,
  editing,
  showSyncOps,
  err,
  onSubmitConnect,
  onSaveOnly,
  onQuick,
  onClose,
}: Props) {
  const patch = (p: Partial<SessionDraft>) => onChange({ ...draft, ...p });

  return (
    <div className="settings-page">
      <form
        className="settings-inner"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmitConnect();
        }}
      >
        <header className="about-hero">
          <span className="logo">Fs</span>
          <div className="about-hero-text">
            <h1>{title}</h1>
            <p>{editing ? "修改该连接的全部信息" : "填写主机、认证与分组后保存"}</p>
          </div>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </header>

        <section>
          <h2>连接</h2>
          <label>
            名称
            <input value={draft.name} onChange={(e) => patch({ name: e.target.value })} placeholder="主机名字" />
          </label>
          <label>
            备注
            <input value={draft.notes} onChange={(e) => patch({ notes: e.target.value })} placeholder="可选，用于标签和列表副标题" />
          </label>
          <label>
            分组
            <select value={draft.folder_id} onChange={(e) => patch({ folder_id: e.target.value })}>
              <option value="">未分组</option>
              {walkFolders(folders).map(({ folder: f, depth }) => (
                <option key={f.id} value={f.id}>
                  {`${"— ".repeat(depth)}${f.name}`}
                </option>
              ))}
            </select>
          </label>
          <label>
            主机
            <input value={draft.host} onChange={(e) => patch({ host: e.target.value })} required />
          </label>
          <div className="row">
            <label>
              端口
              <input type="number" value={draft.port} onChange={(e) => patch({ port: Number(e.target.value) })} />
            </label>
            <label>
              用户
              <input value={draft.username} onChange={(e) => patch({ username: e.target.value })} />
            </label>
          </div>
          <label>
            认证方式
            <select value={draft.auth_method} onChange={(e) => patch({ auth_method: e.target.value as AuthKind })}>
              <option value="password">密码</option>
              <option value="key">私钥</option>
              <option value="agent">Pageant / ssh-agent</option>
              <option value="keyboard">键盘交互</option>
            </select>
          </label>
          {(draft.auth_method === "password" || draft.auth_method === "keyboard") && (
            <label>
              密码
              <input
                type="password"
                value={draft.password}
                onChange={(e) => patch({ password: e.target.value })}
                placeholder={editing ? "留空则不修改已存密码" : ""}
              />
            </label>
          )}
          {draft.auth_method === "key" && (
            <>
              <label>
                已保存密钥
                <select value={draft.key_id} onChange={(e) => patch({ key_id: e.target.value })}>
                  <option value="">选择密钥…</option>
                  {keys.map((k) => (
                    <option key={k.id} value={k.id}>
                      {k.name} · {k.fingerprint.slice(0, 24)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                密钥口令
                <input type="password" value={draft.passphrase} onChange={(e) => patch({ passphrase: e.target.value })} />
              </label>
            </>
          )}
          {draft.auth_method === "agent" && <p className="hint">Windows 使用 Pageant；其他系统使用 SSH_AUTH_SOCK。</p>}
          <label>
            跳板机
            <select value={draft.jump_host_id} onChange={(e) => patch({ jump_host_id: e.target.value })}>
              <option value="">无</option>
              {sessions
                .filter((s) => s.in_ops !== false)
                .map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.host})
                  </option>
                ))}
            </select>
          </label>
          <label>
            算法配置
            <select value={draft.profile} onChange={(e) => patch({ profile: e.target.value as Profile })}>
              <option value="modern">现代（默认，禁用 3DES/SHA1）</option>
              <option value="compatible">兼容（AES-CBC）</option>
              <option value="legacy">老旧设备（3DES / hmac-sha1 / ssh-rsa）</option>
            </select>
          </label>
          {showSyncOps && (
            <label className="check-row">
              <input type="checkbox" checked={draft.in_ops} onChange={(e) => patch({ in_ops: e.target.checked })} />
              同步到常规运维
            </label>
          )}
          {err && <div className="error">{err}</div>}
          <div className="row card-actions">
            <button type="submit" className="primary">
              {editing ? "保存" : "保存并连接"}
            </button>
            {!editing && (
              <button type="button" onClick={onSaveOnly}>
                仅保存
              </button>
            )}
            {!editing && onQuick && (
              <button type="button" onClick={onQuick}>
                快速连接
              </button>
            )}
            <button type="button" onClick={onClose}>
              取消
            </button>
          </div>
        </section>
      </form>
    </div>
  );
}
