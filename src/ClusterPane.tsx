import { useEffect, useMemo, useState } from "react";
import { api } from "./api";
import AiPanel from "./AiPanel";
import type { ClusterProject, HostMetrics, SavedSession } from "./types";

type Props = {
  project: ClusterProject;
  nodes: SavedSession[];
  liveMap: Record<string, string>;
  aiOn: boolean;
  aiWidth: number;
  onAiWidth: (w: number) => void;
  onConnect: (s: SavedSession) => void;
  onAddFromOps: () => void;
  onAddNew: () => void;
};

function pct(used: number, total: number) {
  if (!total) return "—";
  return `${Math.round((used / total) * 100)}%`;
}

export default function ClusterPane({
  project,
  nodes,
  liveMap,
  aiOn,
  aiWidth,
  onAiWidth,
  onConnect,
  onAddFromOps,
  onAddNew,
}: Props) {
  const [metrics, setMetrics] = useState<Record<string, HostMetrics>>({});
  const running = nodes.filter((n) => liveMap[n.id]).length;
  const clusterId = `cluster:${project.id}`;

  useEffect(() => {
    let stop = false;
    async function tick() {
      const next: Record<string, HostMetrics> = {};
      await Promise.all(
        nodes.map(async (n) => {
          const live = liveMap[n.id];
          if (!live) return;
          try {
            next[n.id] = await api.hostMonitor(live);
          } catch {
            /* offline */
          }
        }),
      );
      if (!stop) setMetrics(next);
    }
    void tick();
    const id = window.setInterval(() => void tick(), 4000);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, [nodes, liveMap]);

  const avgCpu = useMemo(() => {
    const vals = Object.values(metrics).map((m) => m.cpu_percent);
    if (!vals.length) return 0;
    return vals.reduce((a, b) => a + b, 0) / vals.length;
  }, [metrics]);

  const mem = useMemo(() => {
    let used = 0;
    let total = 0;
    for (const m of Object.values(metrics)) {
      used += m.mem_used_kb;
      total += m.mem_total_kb;
    }
    return { used, total };
  }, [metrics]);

  const liveNodes = nodes
    .filter((n) => liveMap[n.id])
    .map((n) => [n.id, liveMap[n.id], n.name || n.host] as [string, string, string]);

  return (
    <div className={`cluster-pane${aiOn ? " with-ai" : ""}`}>
      <div className="cluster-main">
        <header className="cluster-head">
          <div>
            <h2>{project.name}</h2>
            <p className="hint">{project.notes.trim() || "集群总览与公共 AI 助手"}</p>
          </div>
          <div className="row">
            <button type="button" onClick={onAddFromOps}>
              从常规运维选择
            </button>
            <button type="button" className="primary" onClick={onAddNew}>
              新增节点
            </button>
          </div>
        </header>
        <div className="cluster-cards">
          <div className="cluster-card">
            <small>节点</small>
            <b>{nodes.length}</b>
          </div>
          <div className="cluster-card">
            <small>运行中</small>
            <b>{running}</b>
          </div>
          <div className="cluster-card">
            <small>平均 CPU</small>
            <b>{running ? `${avgCpu.toFixed(1)}%` : "—"}</b>
          </div>
          <div className="cluster-card">
            <small>内存占用</small>
            <b>{running ? pct(mem.used, mem.total) : "—"}</b>
          </div>
        </div>
        <ul className="cluster-nodes">
          {nodes.length === 0 && <li className="muted">还没有节点。可从常规运维选择，或新增连接。</li>}
          {nodes.map((n) => {
            const m = metrics[n.id];
            const live = !!liveMap[n.id];
            return (
              <li key={n.id}>
                <div>
                  <b>{n.name || n.host}</b>
                  <small>
                    {n.username}@{n.host}:{n.port}
                  </small>
                </div>
                <span className={`dot${live ? " pulse" : ""}`} />
                <span className="muted">{live ? "运行中" : "未连接"}</span>
                {m && (
                  <span className="muted">
                    CPU {m.cpu_percent.toFixed(0)}% · 内存 {pct(m.mem_used_kb, m.mem_total_kb)}
                  </span>
                )}
                <button type="button" onClick={() => onConnect(n)}>
                  {live ? "打开终端" : "连接"}
                </button>
              </li>
            );
          })}
        </ul>
      </div>
      {aiOn && (
        <AiPanel
          sessionId={clusterId}
          width={aiWidth}
          onWidth={onAiWidth}
          visible
          clusterNodes={liveNodes}
        />
      )}
    </div>
  );
}
