import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { FirewallMetric, HostMetrics, NetMetric } from "./types";
import PageLoading from "./Loading";

type Props = { sessionId: string };

function fmtBytes(bytes: number) {
  const b = Math.max(0, bytes);
  if (b < 1024) return `${b.toFixed(0)} B`;
  if (b < 1024 ** 2) return `${(b / 1024).toFixed(1)} KB`;
  if (b < 1024 ** 3) return `${(b / 1024 ** 2).toFixed(1)} MB`;
  return `${(b / 1024 ** 3).toFixed(2)} GB`;
}

function fmtBytesFromKb(kb: number) {
  return fmtBytes(kb * 1024);
}

function fmtRate(bps: number) {
  return `${fmtBytes(bps)}/s`;
}

function fmtUptime(secs: number) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d} 天 ${h} 小时`;
  if (h > 0) return `${h} 小时 ${m} 分`;
  return `${m} 分钟`;
}

function Bar({ value, warn }: { value: number; warn?: number }) {
  const pct = Math.max(0, Math.min(100, value));
  const hot = pct >= (warn ?? 90);
  return (
    <div className="meter">
      <div className={hot ? "fill hot" : "fill"} style={{ width: `${pct}%` }} />
    </div>
  );
}

function fwKind(f: FirewallMetric): "on" | "off" | "none" {
  if (!f.present) return "none";
  return f.active ? "on" : "off";
}

function fwLabel(kind: "on" | "off" | "none") {
  if (kind === "none") return "无";
  if (kind === "on") return "有 · 开启";
  return "有 · 关闭";
}

function isLoop(name: string) {
  return name === "lo" || name.startsWith("lo:");
}

function netDelta(curr: NetMetric[], prev: NetMetric[] | undefined, iface: string) {
  const a = curr.find((n) => n.iface === iface);
  const b = prev?.find((n) => n.iface === iface);
  if (!a) return { rxRate: 0, txRate: 0 };
  if (!b) return { rxRate: 0, txRate: 0 };
  return {
    rxRate: Math.max(0, a.rx_bytes - b.rx_bytes) / 2.5,
    txRate: Math.max(0, a.tx_bytes - b.tx_bytes) / 2.5,
  };
}

function LogBlock({ title, lines, empty }: { title: string; lines: string[]; empty: string }) {
  return (
    <section>
      <h3>{title}</h3>
      {lines.length === 0 ? (
        <p className="muted">{empty}</p>
      ) : (
        <pre className="mon-log">{lines.join("\n")}</pre>
      )}
    </section>
  );
}

function FoldLog({
  title,
  empty,
  sessionId,
  kind,
}: {
  title: string;
  empty: string;
  sessionId: string;
  kind: "login" | "auth";
}) {
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<string[] | null>(null);
  const [err, setErr] = useState("");

  useEffect(() => {
    if (!open) return;
    let stop = false;
    async function tick() {
      try {
        const rows = kind === "login" ? await api.hostLoginHistory(sessionId) : await api.hostAuthLog(sessionId);
        if (stop) return;
        setLines(rows);
        setErr("");
      } catch (e) {
        if (!stop) setErr(String(e));
      }
    }
    tick();
    const id = setInterval(tick, 2500);
    return () => {
      stop = true;
      clearInterval(id);
    };
  }, [open, sessionId, kind]);

  return (
    <section>
      <button type="button" className="mon-fold" onClick={() => setOpen((v) => !v)}>
        <span className="chev">{open ? "▾" : "▸"}</span>
        <h3>{title}</h3>
      </button>
      {open && err && <div className="banner">{err}</div>}
      {open && !err && lines === null && <p className="muted">正在获取…</p>}
      {open && !err && lines && lines.length === 0 && <p className="muted">{empty}</p>}
      {open && !err && lines && lines.length > 0 && <pre className="mon-log">{lines.join("\n")}</pre>}
    </section>
  );
}

export default function MonitorPane({ sessionId }: Props) {
  const [data, setData] = useState<HostMetrics | null>(null);
  const [err, setErr] = useState("");
  const prevRef = useRef<HostMetrics | null>(null);
  const [prevNet, setPrevNet] = useState<HostMetrics | null>(null);

  useEffect(() => {
    let stop = false;
    async function tick() {
      try {
        const m = await api.hostMonitor(sessionId);
        if (stop) return;
        setPrevNet(prevRef.current);
        prevRef.current = m;
        setData(m);
        setErr("");
      } catch (e) {
        if (!stop) setErr(String(e));
      }
    }
    tick();
    const id = setInterval(tick, 2500);
    return () => {
      stop = true;
      clearInterval(id);
    };
  }, [sessionId]);

  const cpu = data?.cpu_percent ?? 0;
  const memPct = data && data.mem_total_kb ? (data.mem_used_kb / data.mem_total_kb) * 100 : 0;
  const swapPct = data && data.swap_total_kb ? (data.swap_used_kb / data.swap_total_kb) * 100 : 0;
  const dataNets = data?.nets ?? [];
  const wan = dataNets.filter((n) => !isLoop(n.iface));
  const rxTotal = wan.reduce((s, n) => s + n.rx_bytes, 0);
  const txTotal = wan.reduce((s, n) => s + n.tx_bytes, 0);
  const rxRateTotal = wan.reduce((s, n) => s + netDelta(dataNets, prevNet?.nets, n.iface).rxRate, 0);
  const txRateTotal = wan.reduce((s, n) => s + netDelta(dataNets, prevNet?.nets, n.iface).txRate, 0);

  return (
    <div className="monitor">
      {err && <div className="banner">{err}</div>}
      {!data && !err && <PageLoading text="正在采集主机指标…" />}
      {data && (
        <>
          <section>
            <h3>服务器信息</h3>
            <dl className="mon-info">
              <dt>主机名</dt>
              <dd>{data.hostname || "—"}</dd>
              <dt>操作系统</dt>
              <dd>{data.os_name || data.uname || "—"}</dd>
              <dt>内核</dt>
              <dd>{data.kernel || data.uname || "—"}</dd>
              <dt>架构</dt>
              <dd>{data.arch || "—"}</dd>
              <dt>CPU</dt>
              <dd>
                {data.cpu_model || "—"}
                {data.cpu_cores ? ` · ${data.cpu_cores} 核` : ""}
              </dd>
              <dt>虚拟化</dt>
              <dd>{data.virt && data.virt !== "unknown" ? data.virt : "—"}</dd>
              <dt>运行时间</dt>
              <dd>{fmtUptime(data.uptime_secs)}</dd>
              <dt>负载</dt>
              <dd>
                {data.load1.toFixed(2)} / {data.load5.toFixed(2)} / {data.load15.toFixed(2)}
              </dd>
            </dl>
            <small className="muted">每 2.5 秒刷新</small>
          </section>
          <div className="mon-grid">
            <section>
              <h3>CPU</h3>
              <div className="stat">{cpu.toFixed(1)}%</div>
              <Bar value={cpu} />
            </section>
            <section>
              <h3>内存</h3>
              <div className="stat">{memPct.toFixed(1)}%</div>
              <Bar value={memPct} />
              <small>
                已用 {fmtBytesFromKb(data.mem_used_kb)} / {fmtBytesFromKb(data.mem_total_kb)}
              </small>
            </section>
            <section>
              <h3>交换分区</h3>
              <div className="stat">{data.swap_total_kb ? `${swapPct.toFixed(1)}%` : "未配置"}</div>
              {data.swap_total_kb > 0 && <Bar value={swapPct} />}
              <small>
                {fmtBytesFromKb(data.swap_used_kb)} / {fmtBytesFromKb(data.swap_total_kb)}
              </small>
            </section>
          </div>
          <section>
            <h3>磁盘</h3>
            <table>
              <thead>
                <tr>
                  <th>挂载点</th>
                  <th>设备</th>
                  <th>已用</th>
                  <th>容量</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {data.disks.map((d) => {
                  const pct = d.total_kb ? (d.used_kb / d.total_kb) * 100 : 0;
                  return (
                    <tr key={d.mount + d.source}>
                      <td>{d.mount}</td>
                      <td className="muted">{d.source}</td>
                      <td>{fmtBytesFromKb(d.used_kb)}</td>
                      <td>{fmtBytesFromKb(d.total_kb)}</td>
                      <td className="bar-cell">
                        <Bar value={pct} warn={85} />
                        <span>{d.percent}</span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>
          <section>
            <h3>网卡</h3>
            <table>
              <thead>
                <tr>
                  <th>接口</th>
                  <th>状态</th>
                  <th>MAC</th>
                  <th>MTU</th>
                  <th>IPv4</th>
                </tr>
              </thead>
              <tbody>
                {data.nics.map((n) => (
                  <tr key={n.name}>
                    <td>{n.name}</td>
                    <td>{n.state}</td>
                    <td className="muted">{n.mac}</td>
                    <td>{n.mtu}</td>
                    <td>{n.ipv4 || "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
          <section>
            <h3>网络流量</h3>
            <div className="mon-grid net-summary">
              <div>
                <h4>入站</h4>
                <div className="stat">{fmtRate(rxRateTotal)}</div>
                <small>累计 {fmtBytes(rxTotal)}</small>
              </div>
              <div>
                <h4>出站</h4>
                <div className="stat">{fmtRate(txRateTotal)}</div>
                <small>累计 {fmtBytes(txTotal)}</small>
              </div>
            </div>
            <table className="net-table">
              <thead>
                <tr>
                  <th rowSpan={2}>接口</th>
                  <th colSpan={4}>入站</th>
                  <th colSpan={4}>出站</th>
                </tr>
                <tr>
                  <th>累计</th>
                  <th>速率</th>
                  <th>包</th>
                  <th>丢包</th>
                  <th>累计</th>
                  <th>速率</th>
                  <th>包</th>
                  <th>丢包</th>
                </tr>
              </thead>
              <tbody>
                {data.nets.map((n) => {
                  const { rxRate, txRate } = netDelta(data.nets, prevNet?.nets, n.iface);
                  return (
                    <tr key={n.iface}>
                      <td>{n.iface}</td>
                      <td>{fmtBytes(n.rx_bytes)}</td>
                      <td>{fmtRate(rxRate)}</td>
                      <td>{n.rx_packets.toLocaleString()}</td>
                      <td>{n.rx_drop.toLocaleString()}</td>
                      <td>{fmtBytes(n.tx_bytes)}</td>
                      <td>{fmtRate(txRate)}</td>
                      <td>{n.tx_packets.toLocaleString()}</td>
                      <td>{n.tx_drop.toLocaleString()}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <small className="muted">汇总不含回环接口 lo</small>
          </section>
          <section>
            <h3>防火墙</h3>
            <ul className="fw">
              {data.firewall.map((f) => {
                const kind = fwKind(f);
                return (
                  <li key={f.name}>
                    <b>{f.name}</b>
                    <span className={`fw-pill ${kind}`}>{fwLabel(kind)}</span>
                  </li>
                );
              })}
            </ul>
          </section>
          <LogBlock title="当前登录" lines={data.sessions ?? []} empty="当前没有交互式登录会话。" />
          <FoldLog
            title="登录历史"
            empty="暂无登录历史（wtmp 不可读或不存在）。"
            sessionId={sessionId}
            kind="login"
          />
          <FoldLog
            title="SSH 连接日志"
            empty="暂无 SSH 认证记录（可能无权限读取 /var/log/secure 或 auth.log）。"
            sessionId={sessionId}
            kind="auth"
          />
        </>
      )}
    </div>
  );
}
