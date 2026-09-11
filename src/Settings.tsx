import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { DEFAULT_TERM_FG_HEX, EYE_CARE_FGS } from "./termTheme";
import type { AppSettings } from "./types";

const THEMES = [
  { id: "ink", name: "深海墨青", swatch: "#0b0f14" },
  { id: "graphite", name: "石墨灰", swatch: "#111111" },
  { id: "navy", name: "夜空蓝", swatch: "#070b16" },
  { id: "pine", name: "松烟绿", swatch: "#0a100c" },
] as const;

type Props = {
  settings: AppSettings;
  onChange: (next: AppSettings) => void;
  onImported: () => Promise<void>;
  onClose: () => void;
};

export default function SettingsPage({ settings, onChange, onImported, onClose }: Props) {
  async function persist(next: AppSettings) {
    onChange(next);
    await api.saveSettings(next);
  }

  return (
    <div className="settings-page">
      <form
        className="settings-inner"
        onSubmit={async (e) => {
          e.preventDefault();
          await api.saveSettings(settings);
          onClose();
        }}
      >
        <header className="about-hero">
          <span className="logo">Fs</span>
          <div className="about-hero-text">
            <h1>设置</h1>
            <p>外观、终端与本机保险库</p>
          </div>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </header>

        <section>
          <h2>外观</h2>
          <div className="themes">
            {THEMES.map((t) => (
              <button
                key={t.id}
                type="button"
                className={`theme-swatch${(settings.theme || "ink") === t.id ? " active" : ""}`}
                style={{ background: t.swatch }}
                onClick={() => void persist({ ...settings, theme: t.id })}
              >
                {t.name}
              </button>
            ))}
          </div>
        </section>

        <section>
          <h2>终端默认字体（护眼）</h2>
          <p className="hint">
            只替换未着色的默认白字。ls、vim、htop 等程序输出的红/绿/黄等特殊颜色不会改变。
          </p>
          <div className="eye-swatches">
            {EYE_CARE_FGS.map((c) => {
              const on = (settings.term_fg || DEFAULT_TERM_FG_HEX).toLowerCase() === c.color;
              return (
                <button
                  key={c.id}
                  type="button"
                  className={`eye-swatch${on ? " active" : ""}`}
                  onClick={() => void persist({ ...settings, term_fg: c.color })}
                >
                  <span className="eye-preview" style={{ color: c.color }}>
                    Aa 示例
                  </span>
                  <span>{c.name}</span>
                </button>
              );
            })}
          </div>
        </section>

        <section>
          <h2>终端与传输</h2>
          <div className="settings-grid">
            <label>
              字体大小
              <input
                type="number"
                value={settings.font_size}
                onChange={(e) => onChange({ ...settings, font_size: Number(e.target.value) })}
              />
            </label>
            <label>
              回滚行数
              <input
                type="number"
                value={settings.scrollback}
                onChange={(e) => onChange({ ...settings, scrollback: Number(e.target.value) })}
              />
            </label>
            <label>
              并行传输
              <input
                type="number"
                value={settings.parallel_transfers}
                onChange={(e) => onChange({ ...settings, parallel_transfers: Number(e.target.value) })}
              />
            </label>
            <label>
              分块 (KiB)
              <input
                type="number"
                value={settings.chunk_kib}
                onChange={(e) => onChange({ ...settings, chunk_kib: Number(e.target.value) })}
              />
            </label>
          </div>
        </section>

        <section>
          <h2>保险库</h2>
          <div className="row">
            <button
              type="button"
              onClick={async () => {
                const path = await save({
                  defaultPath: "ferrassh-vault.bin",
                  filters: [{ name: "Vault", extensions: ["bin"] }],
                });
                if (path) await api.exportVault(path);
              }}
            >
              导出保险库
            </button>
            <button
              type="button"
              onClick={async () => {
                const path = await open({ filters: [{ name: "Vault", extensions: ["bin"] }] });
                if (typeof path === "string") {
                  await api.importVault(path);
                  await onImported();
                }
              }}
            >
              导入保险库
            </button>
          </div>
        </section>

        <section>
          <h2>操作日志</h2>
          <p className="hint">日志按天写在安装目录 logs 文件夹，打开软件时会异步清掉过期文件。</p>
          <label>
            保留天数
            <input
              type="number"
              min={1}
              value={settings.log_retain_days ?? 5}
              onChange={(e) => onChange({ ...settings, log_retain_days: Math.max(1, Number(e.target.value) || 5) })}
            />
          </label>
        </section>

        <div className="row card-actions">
          <button type="submit" className="primary">
            保存
          </button>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </div>
      </form>
    </div>
  );
}
