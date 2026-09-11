import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  ClusterProject,
  Folder,
  HostMetrics,
  ProjectNode,
  RemoteEntry,
  SavedKey,
  SavedSession,
  SyncPlan,
  TermFrame,
} from "./types";

export type AiSettings = {
  enabled: boolean;
  provider: string;
  model: string;
  api_key: string;
  base_url: string;
  danger_commands?: string[];
};

export type PathStat = { exists: boolean; is_dir: boolean; path: string; name: string; size: number; mtime: number };

export type KbSourceSummary = {
  source: string;
  count: number;
  titles: string[];
};

export type KbDoc = {
  id: string;
  source: string;
  title: string;
  body: string;
  tags: string;
  created_at: number;
};

export type AiModel = { id: string; name: string };

export type AiProvider = {
  id: string;
  name: string;
  base_url: string;
  models: AiModel[];
  needs_key: boolean;
  hint: string;
};

export const api = {
  vaultStatus: () => invoke<{ initialized: boolean; unlocked: boolean }>("vault_status"),
  vaultInit: (password: string) => invoke<void>("vault_init", { password }),
  vaultUnlock: (password: string) => invoke<void>("vault_unlock", { password }),
  vaultLock: () => invoke<void>("vault_lock"),
  listFolders: () => invoke<Folder[]>("list_folders"),
  upsertFolder: (folder: Folder) => invoke<void>("upsert_folder", { folder }),
  deleteFolder: (id: string) => invoke<void>("delete_folder", { id }),
  listSessions: () => invoke<SavedSession[]>("list_sessions"),
  upsertSession: (session: SavedSession, password?: string, passphrase?: string) =>
    invoke<void>("upsert_session", { session, password: password ?? null, passphrase: passphrase ?? null }),
  deleteSession: (id: string) => invoke<void>("delete_session", { id }),
  listKeys: () => invoke<SavedKey[]>("list_keys"),
  importKey: (name: string, pem: string) => invoke<SavedKey>("import_key", { name, pem }),
  generateKey: (name: string) => invoke<SavedKey>("generate_key", { name }),
  deleteKey: (id: string) => invoke<void>("delete_key", { id }),
  exportKeys: (password: string, path: string) => invoke<void>("export_keys", { password, path }),
  importKeys: (path: string) => invoke<number>("import_keys", { path }),
  settings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<void>("save_settings", { settings }),
  catalog: () => invoke<Record<string, unknown>>("algorithm_catalog"),
  connectSaved: (id: string, cols: number, rows: number, acceptUnknown: boolean) =>
    invoke<{ session_id: string; label: string; fingerprint: string }>("connect_saved", {
      id,
      cols,
      rows,
      acceptUnknown,
    }),
  connectQuick: (req: Record<string, unknown>) =>
    invoke<{ session_id: string; label: string; fingerprint: string }>("connect_quick", { req }),
  termWrite: (sessionId: string, data: number[]) => invoke<void>("term_write", { sessionId, data }),
  termWriteText: (sessionId: string, text: string) => invoke<void>("term_write_text", { sessionId, text }),
  termResize: (sessionId: string, cols: number, rows: number) =>
    invoke<TermFrame>("term_resize", { sessionId, cols, rows }),
  termScroll: (sessionId: string, delta: number) => invoke<TermFrame>("term_scroll", { sessionId, delta }),
  termScrollTo: (sessionId: string, offset: number) => invoke<TermFrame>("term_scroll_to", { sessionId, offset }),
  termRangeText: (sessionId: string, startLine: number, startCol: number, endLine: number, endCol: number) =>
    invoke<string>("term_range_text", { sessionId, startLine, startCol, endLine, endCol }),
  termCwd: (sessionId: string) => invoke<string>("term_cwd", { sessionId }),
  termFrame: (sessionId: string) => invoke<TermFrame>("term_frame", { sessionId }),
  hostMonitor: (sessionId: string) => invoke<HostMetrics>("host_monitor", { sessionId }),
  hostLoginHistory: (sessionId: string) => invoke<string[]>("host_login_history", { sessionId }),
  hostAuthLog: (sessionId: string) => invoke<string[]>("host_auth_log", { sessionId }),
  disconnect: (sessionId: string) => invoke<void>("disconnect", { sessionId }),
  sftpList: (sessionId: string, path: string) => invoke<RemoteEntry[]>("sftp_list", { sessionId, path }),
  sftpMkdir: (sessionId: string, path: string) => invoke<void>("sftp_mkdir", { sessionId, path }),
  sftpCreateFile: (sessionId: string, path: string) => invoke<void>("sftp_create_file", { sessionId, path }),
  sftpRemove: (sessionId: string, path: string, recursive: boolean) =>
    invoke<void>("sftp_remove", { sessionId, path, recursive }),
  sftpRename: (sessionId: string, from: string, to: string) => invoke<void>("sftp_rename", { sessionId, from, to }),
  sftpChmod: (sessionId: string, path: string, mode: number) => invoke<void>("sftp_chmod", { sessionId, path, mode }),
  sftpTransfer: (sessionId: string, local: string, remote: string, upload: boolean, resume: boolean, overwrite = false) =>
    invoke<number>("sftp_transfer", { sessionId, local, remote, upload, resume, overwrite }),
  sftpStat: (sessionId: string, path: string) => invoke<PathStat>("sftp_stat", { sessionId, path }),
  localStat: (path: string) => invoke<PathStat>("local_stat", { path }),
  sftpSyncPlan: (sessionId: string, local: string, remote: string, bidirectional: boolean) =>
    invoke<SyncPlan>("sftp_sync_plan", { sessionId, local, remote, bidirectional }),
  sftpSyncApply: (sessionId: string, local: string, remote: string, plan: SyncPlan) =>
    invoke<number>("sftp_sync_apply", { sessionId, local, remote, plan }),
  listLocal: (path: string) => invoke<RemoteEntry[]>("list_local", { path }),
  mkdirLocal: (path: string) => invoke<void>("mkdir_local", { path }),
  createLocalFile: (path: string) => invoke<void>("create_local_file", { path }),
  removeLocal: (path: string, sessionId: string) => invoke<void>("remove_local", { path, sessionId }),
  exportVault: (path: string) => invoke<void>("export_vault", { path }),
  importVault: (path: string) => invoke<void>("import_vault", { path }),
  aiCatalog: () => invoke<AiProvider[]>("ai_catalog"),
  aiSettings: () => invoke<AiSettings>("get_ai_settings"),
  saveAiSettings: (settings: AiSettings) => invoke<void>("save_ai_settings", { settings }),
  aiKbSources: () => invoke<KbSourceSummary[]>("ai_kb_sources"),
  aiKbList: (source: string) => invoke<KbDoc[]>("ai_kb_list", { source }),
  aiKbSave: (oldSource: string, newSource: string, docs: KbDoc[]) =>
    invoke<void>("ai_kb_save", { oldSource, newSource, docs }),
  aiKbImport: (path: string) => invoke<number>("ai_kb_import", { path }),
  aiKbDeleteSource: (source: string) => invoke<void>("ai_kb_delete_source", { source }),
  aiKbClear: () => invoke<void>("ai_kb_clear"),
  aiKbSaveTemplate: (path: string) => invoke<void>("ai_kb_save_template", { path }),
  aiPrepare: (sessionId: string) => invoke<string>("ai_prepare", { sessionId }),
  aiAsk: (sessionId: string, text: string) => invoke<void>("ai_ask", { sessionId, text }),
  aiClusterAsk: (sessionId: string, text: string, nodes: [string, string, string][]) =>
    invoke<void>("ai_cluster_ask", { sessionId, text, nodes }),
  aiCancel: (sessionId: string) => invoke<void>("ai_cancel", { sessionId }),
  aiResume: (sessionId: string) => invoke<void>("ai_resume", { sessionId }),
  aiDangerReply: (sessionId: string, approved: boolean, command: string) =>
    invoke<void>("ai_danger_reply", { sessionId, approved, command }),
  listProjects: () => invoke<ClusterProject[]>("list_projects"),
  upsertProject: (project: ClusterProject) => invoke<void>("upsert_project", { project }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  listProjectNodes: () => invoke<ProjectNode[]>("list_project_nodes"),
  addProjectNode: (projectId: string, sessionId: string, sortOrder: number) =>
    invoke<void>("add_project_node", { projectId, sessionId, sortOrder }),
  removeProjectNode: (projectId: string, sessionId: string) =>
    invoke<void>("remove_project_node", { projectId, sessionId }),
  oplogWrite: (actor: string, action: string, detail: string) => invoke<void>("oplog_write", { actor, action, detail }),
  oplogDir: () => invoke<string>("oplog_dir"),
  checkAppUpdate: (current: string) =>
    invoke<{ current: string; latest: string; newer: boolean; notes: string }>("check_app_update", { current }),
  installAppUpdate: (current: string) => invoke<void>("install_app_update", { current }),
  deviceSealOk: () => invoke<boolean>("device_seal_ok"),
  clipboardWrite: (text: string) => invoke<void>("clipboard_write", { text }),
  clipboardRead: () => invoke<string>("clipboard_read"),
};
