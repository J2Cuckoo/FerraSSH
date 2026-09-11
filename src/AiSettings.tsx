import { useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type AiProvider, type AiSettings, type KbDoc, type KbSourceSummary } from "./api";

type Props = {
  onClose: () => void;
  onSaved?: (enabled: boolean) => void;
};

const DISCLAIMER =
  "本平台仅提供 AI 交互通道，不参与也不干预大模型生成内容的审核与判断。大模型输出的内容是否可执行，由用户自行核实并确认；用户因采纳、执行上述内容所产生的一切后果与法律责任，均由用户自行承担，平台不承担任何责任。";

export default function AiSettingsPage({ onClose, onSaved }: Props) {
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [cfg, setCfg] = useState<AiSettings>({
    enabled: false,
    provider: "deepseek",
    model: "deepseek-v4-flash",
    api_key: "",
    base_url: "https://api.deepseek.com",
    danger_commands: [],
  });
  const [dangerDraft, setDangerDraft] = useState("");
  const [sources, setSources] = useState<KbSourceSummary[]>([]);
  const [err, setErr] = useState("");
  const [ok, setOk] = useState("");
  const [editing, setEditing] = useState<{ oldSource: string; source: string; docs: KbDoc[] } | null>(null);
  const [modelOpen, setModelOpen] = useState(false);

  const current = useMemo(() => providers.find((p) => p.id === cfg.provider), [providers, cfg.provider]);
  const modelOptions = current?.models || [];
  const selectedModel = modelOptions.find((m) => m.id === cfg.model);
  const modelText = selectedModel ? selectedModel.name : cfg.model;
  const modelFiltered = useMemo(() => {
    if (selectedModel) return modelOptions;
    const q = cfg.model.trim().toLowerCase();
    if (!q) return modelOptions;
    const hit = modelOptions.filter((m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q));
    return hit.length ? hit : modelOptions;
  }, [cfg.model, modelOptions, selectedModel]);
  const isOllama = cfg.provider === "ollama";

  async function reloadKb() {
    setSources(await api.aiKbSources());
  }

  async function reload() {
    const [list, settings, kb] = await Promise.all([api.aiCatalog(), api.aiSettings(), api.aiKbSources()]);
    setProviders(list);
    const rawProvider = settings.provider || "deepseek";
    const provider = list.some((x) => x.id === rawProvider) ? rawProvider : "deepseek";
    const p = list.find((x) => x.id === provider);
    let model = settings.model || p?.models[0]?.id || "";
    if (provider === "deepseek" && (model === "deepseek-chat" || model === "deepseek-reasoner")) {
      model = model === "deepseek-reasoner" ? "deepseek-v4-pro" : "deepseek-v4-flash";
    }
    if (provider !== rawProvider) {
      model = p?.models[0]?.id || model;
    }
    setCfg({
      enabled: !!settings.enabled,
      provider,
      model,
      api_key: settings.api_key || "",
      base_url: (provider !== rawProvider ? "" : settings.base_url) || p?.base_url || "",
      danger_commands: settings.danger_commands?.length ? settings.danger_commands : [],
    });
    setSources(kb);
  }

  useEffect(() => {
    void reload().catch((e) => setErr(String(e)));
  }, []);

  function patch(next: Partial<AiSettings>) {
    setCfg((c) => ({ ...c, ...next }));
    setErr("");
    setOk("");
  }

  function switchProvider(id: string) {
    const p = providers.find((x) => x.id === id);
    patch({
      provider: id,
      model: p?.models[0]?.id || "",
      base_url: p?.base_url || "",
    });
    setModelOpen(false);
  }

  async function persist(next: AiSettings) {
    setErr("");
    setOk("");
    const payload: AiSettings = {
      ...next,
      base_url: next.base_url.trim() || current?.base_url || "",
    };
    try {
      await api.saveAiSettings(payload);
      setCfg(payload);
      onSaved?.(payload.enabled);
      setOk("已保存");
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div className="settings-page">
      <form
        className="settings-inner"
        onSubmit={(e) => {
          e.preventDefault();
          void persist(cfg);
        }}
      >
        <header className="about-hero">
          <span className="logo">Fs</span>
          <div className="about-hero-text">
            <h1>AI</h1>
            <p>大模型平台、密钥与知识库</p>
          </div>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </header>

        <section>
          <h2>开关</h2>
          <div className="switch-row">
            <span>开启 AI 助手</span>
            <button
              type="button"
              className={`switch${cfg.enabled ? " on" : ""}`}
              role="switch"
              aria-checked={cfg.enabled}
              onClick={() => patch({ enabled: !cfg.enabled })}
            >
              <span className="switch-knob" />
            </button>
          </div>
          <p className="disclaimer">{DISCLAIMER}</p>
        </section>

        <section>
          <h2>模型</h2>
          <div className="settings-grid">
            <label>
              平台
              <select value={cfg.provider} onChange={(e) => switchProvider(e.target.value)}>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              模型
              <div className={`combo${modelOpen ? " open" : ""}`}>
                <input
                  value={modelText}
                  placeholder="选择或输入模型名"
                  autoComplete="off"
                  onFocus={() => setModelOpen(true)}
                  onChange={(e) => {
                    const v = e.target.value;
                    const byName = modelOptions.find((m) => m.name === v);
                    const byId = modelOptions.find((m) => m.id === v);
                    patch({ model: byName?.id || byId?.id || v });
                    setModelOpen(true);
                  }}
                  onBlur={() => window.setTimeout(() => setModelOpen(false), 120)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setModelOpen(false);
                    if (e.key === "ArrowDown") {
                      e.preventDefault();
                      setModelOpen(true);
                    }
                  }}
                />
                <button
                  type="button"
                  className="combo-caret"
                  tabIndex={-1}
                  aria-label="打开模型列表"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    setModelOpen((v) => !v);
                  }}
                />
                {modelOpen && modelFiltered.length > 0 && (
                  <ul className="combo-menu" role="listbox">
                    {modelFiltered.map((m) => (
                      <li key={m.id}>
                        <button
                          type="button"
                          className={m.id === cfg.model ? "on" : ""}
                          onMouseDown={(e) => {
                            e.preventDefault();
                            patch({ model: m.id });
                            setModelOpen(false);
                          }}
                        >
                          <span className="combo-name">{m.name}</span>
                          <span className="combo-id">{m.id}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </label>
            <label className="span-2">
              API Key
              <input
                type="password"
                value={cfg.api_key}
                onChange={(e) => patch({ api_key: e.target.value })}
                placeholder={isOllama ? "Ollama 可不填" : "sk-…"}
                autoComplete="off"
              />
            </label>
            <label className="span-2">
              接口地址
              <input
                value={cfg.base_url}
                onChange={(e) => patch({ base_url: e.target.value })}
                placeholder={current?.base_url || "https://…"}
                required={cfg.enabled && isOllama}
              />
            </label>
          </div>
        </section>

        <section>
          <h2>危险命令</h2>
          <p className="hint">匹配到这些片段的命令不会直接执行，必须由你在中央弹窗中同意或拒绝。可增删。</p>
          <ul className="danger-list">
            {(cfg.danger_commands || []).map((c) => (
              <li key={c}>
                <code>{c}</code>
                <button
                  type="button"
                  className="icon-btn"
                  onClick={() => patch({ danger_commands: (cfg.danger_commands || []).filter((x) => x !== c) })}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
          <div className="row">
            <input
              value={dangerDraft}
              onChange={(e) => setDangerDraft(e.target.value)}
              placeholder="例如 systemctl restart"
            />
            <button
              type="button"
              onClick={() => {
                const v = dangerDraft.trim();
                if (!v) return;
                const cur = cfg.danger_commands || [];
                if (cur.includes(v)) return;
                patch({ danger_commands: [...cur, v] });
                setDangerDraft("");
              }}
            >
              新增
            </button>
          </div>
        </section>

        <section>
          <h2>知识库（JSON）</h2>
          <p className="hint">可上传多个 JSON。对话时会结合当前主机是内网还是公网，优先采用知识库内容。</p>
          <div className="row">
            <button
              type="button"
              onClick={async () => {
                const path = await open({ filters: [{ name: "JSON", extensions: ["json"] }] });
                if (typeof path !== "string") return;
                try {
                  const n = await api.aiKbImport(path);
                  setOk(`已导入 ${n} 条`);
                  await reloadKb();
                } catch (e) {
                  setErr(String(e));
                }
              }}
            >
              上传 JSON
            </button>
            <button
              type="button"
              onClick={async () => {
                const path = await save({
                  defaultPath: "ferrassh-kb-template.json",
                  filters: [{ name: "JSON", extensions: ["json"] }],
                });
                if (!path) return;
                try {
                  await api.aiKbSaveTemplate(path);
                  setOk("模板已保存");
                } catch (e) {
                  setErr(String(e));
                }
              }}
            >
              下载模板
            </button>
          </div>
          {sources.length === 0 ? (
            <p className="hint">尚未上传知识库。可先下载模板，按 title / tags / content 填写后再上传。</p>
          ) : (
            <ul className="kb-list">
              {sources.map((s) => (
                <li key={s.source}>
                  <div className="kb-meta">
                    <b>{s.source}</b>
                    <small>
                      {s.count} 条
                      {s.titles.length ? ` · ${s.titles.join("、")}` : ""}
                    </small>
                  </div>
                  <div className="kb-actions">
                    <button
                      type="button"
                      onClick={async () => {
                        const docs = await api.aiKbList(s.source);
                        setEditing({
                          oldSource: s.source,
                          source: s.source,
                          docs: docs.length
                            ? docs
                            : [{ id: "", source: s.source, title: "", body: "", tags: "", created_at: 0 }],
                        });
                      }}
                    >
                      修改
                    </button>
                    <button
                      type="button"
                      onClick={async () => {
                        await api.aiKbDeleteSource(s.source);
                        await reloadKb();
                      }}
                    >
                      删除
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        {err && <div className="error">{err}</div>}
        {ok && <p className="hint">{ok}</p>}
        <div className="row card-actions">
          <button type="submit" className="primary">
            保存
          </button>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </div>
      </form>

      {editing && (
        <div className="modal" onClick={() => setEditing(null)}>
          <form
            className="card wide kb-edit"
            onClick={(e) => e.stopPropagation()}
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await api.aiKbSave(editing.oldSource, editing.source, editing.docs);
                setEditing(null);
                setOk("知识库已更新");
                await reloadKb();
              } catch (ex) {
                setErr(String(ex));
              }
            }}
          >
            <h2>修改知识库</h2>
            <label>
              名称
              <input
                value={editing.source}
                onChange={(e) => setEditing({ ...editing, source: e.target.value })}
              />
            </label>
            {editing.docs.map((d, i) => (
              <div key={d.id || i} className="kb-entry">
                <label>
                  标题
                  <input
                    value={d.title}
                    onChange={(e) => {
                      const docs = editing.docs.slice();
                      docs[i] = { ...d, title: e.target.value };
                      setEditing({ ...editing, docs });
                    }}
                  />
                </label>
                <label>
                  标签
                  <input
                    value={d.tags}
                    onChange={(e) => {
                      const docs = editing.docs.slice();
                      docs[i] = { ...d, tags: e.target.value };
                      setEditing({ ...editing, docs });
                    }}
                    placeholder="nginx, proxy"
                  />
                </label>
                <label>
                  内容
                  <textarea
                    rows={4}
                    value={d.body}
                    onChange={(e) => {
                      const docs = editing.docs.slice();
                      docs[i] = { ...d, body: e.target.value };
                      setEditing({ ...editing, docs });
                    }}
                  />
                </label>
                <button
                  type="button"
                  onClick={() =>
                    setEditing({ ...editing, docs: editing.docs.filter((_, j) => j !== i) })
                  }
                >
                  删除此条
                </button>
              </div>
            ))}
            <button
              type="button"
              onClick={() =>
                setEditing({
                  ...editing,
                  docs: [...editing.docs, { id: "", source: editing.source, title: "", body: "", tags: "", created_at: 0 }],
                })
              }
            >
              增加条目
            </button>
            <div className="row card-actions">
              <button type="submit" className="primary">
                保存
              </button>
              <button type="button" onClick={() => setEditing(null)}>
                取消
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
