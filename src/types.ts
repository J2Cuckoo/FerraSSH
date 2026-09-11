export type AlgSpec = {
  profile: "modern" | "compatible" | "legacy" | "custom";
  kex: string[];
  cipher: string[];
  mac: string[];
  host_key: string[];
};

export type SavedSession = {
  id: string;
  folder_id: string | null;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_method: "password" | "key" | "agent" | "keyboard";
  key_id: string | null;
  jump_host_id: string | null;
  algs: AlgSpec;
  local_echo: boolean;
  keepalive: number;
  compression: boolean;
  term: string;
  notes: string;
  sort_order: number;
  updated_at: number;
  has_secret: boolean;
  in_ops?: boolean;
  sftp_local_path?: string;
};

export type SavedKey = {
  id: string;
  name: string;
  public_key: string;
  fingerprint: string;
  created_at: number;
};

export type Folder = { id: string; parent_id: string | null; name: string; sort_order: number; kind?: string };

export type ClusterProject = {
  id: string;
  folder_id: string | null;
  name: string;
  notes: string;
  sort_order: number;
  updated_at: number;
};

export type ProjectNode = { project_id: string; session_id: string; sort_order: number };

export type TermCell = { ch: string; fg: number; bg: number; flags: number };
export type TermLine = { y: number; cells: TermCell[] };
export type TermFrame = {
  cols: number;
  rows: number;
  cursor_x: number;
  cursor_y: number;
  cursor_visible: boolean;
  app_cursor: boolean;
  app_keypad: boolean;
  bracketed_paste: boolean;
  mouse_sgr: boolean;
  mouse_mode: boolean;
  title: string;
  cwd: string;
  lines: TermLine[];
  scroll_offset?: number;
  scroll_max?: number;
};

export type RemoteEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  mode: number;
  mtime: number;
  longname: string;
};

export type TransferProgress = {
  job_id: string;
  path: string;
  transferred: number;
  total: number;
  direction: "upload" | "download" | "delete";
  finished: boolean;
  error: string | null;
  session_id?: string;
};

export type AppSettings = {
  idle_lock_secs: number;
  default_term: string;
  font_family: string;
  font_size: number;
  scrollback: number;
  parallel_transfers: number;
  chunk_kib: number;
  preserve_perms: boolean;
  follow_symlinks: boolean;
  sync_url: string | null;
  sync_account: string | null;
  theme: string;
  term_fg: string;
  log_retain_days?: number;
  minio_endpoint?: string;
  minio_bucket?: string;
  minio_access_key?: string;
  minio_secret_key?: string;
  minio_region?: string;
  minio_object?: string;
};

export type SyncItem = { relative: string; action: "upload" | "download" | "skip" | "link"; reason: string; size: number };
export type SyncPlan = { items: SyncItem[]; uploads: number; downloads: number; skipped: number };

export type DiskMetric = { mount: string; source: string; total_kb: number; used_kb: number; percent: string };
export type NetMetric = {
  iface: string;
  rx_bytes: number;
  tx_bytes: number;
  rx_packets: number;
  tx_packets: number;
  rx_drop: number;
  tx_drop: number;
};
export type NicMetric = { name: string; state: string; mac: string; mtu: number; ipv4: string };
export type FirewallMetric = { name: string; present: boolean; active: boolean; detail: string };
export type HostMetrics = {
  hostname: string;
  uname: string;
  os_name: string;
  kernel: string;
  arch: string;
  cpu_model: string;
  virt: string;
  uptime_secs: number;
  cpu_percent: number;
  cpu_cores: number;
  load1: number;
  load5: number;
  load15: number;
  mem_total_kb: number;
  mem_available_kb: number;
  mem_used_kb: number;
  swap_total_kb: number;
  swap_used_kb: number;
  disks: DiskMetric[];
  nets: NetMetric[];
  nics: NicMetric[];
  firewall: FirewallMetric[];
  sessions: string[];
};

export const emptyAlgs = (): AlgSpec => ({
  profile: "modern",
  kex: [],
  cipher: [],
  mac: [],
  host_key: [],
});
