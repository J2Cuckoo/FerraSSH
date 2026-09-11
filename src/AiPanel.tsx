import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { FocusOwner } from "./focusOwner";

type Msg = { role: "user" | "assistant" | "system"; text: string };

type Props = {
  sessionId: string;
  width: number;
  onWidth: (w: number) => void;
  visible?: boolean;
  clusterNodes?: [string, string, string][];
};

type AiEvent = { session_id: string; kind: string; role: string; text: string };

function AiMark() {
  return (
    <span className="ai-avatar bot" aria-hidden>
      <svg viewBox="0 0 24 24" overflow="visible">
        <path
          fill="currentColor"
          d="M12 3.2a1.8 1.8 0 0 1 1.8 1.8v.9h1.7A2.6 2.6 0 0 1 18.1 8.5v7.2a2.6 2.6 0 0 1-2.6 2.6H8.5a2.6 2.6 0 0 1-2.6-2.6V8.5A2.6 2.6 0 0 1 8.5 5.9h1.7V5A1.8 1.8 0 0 1 12 3.2Zm-2.6 7.2a1.15 1.15 0 1 0 0 2.3 1.15 1.15 0 0 0 0-2.3Zm5.2 0a1.15 1.15 0 1 0 0 2.3 1.15 1.15 0 0 0 0-2.3Z"
        />
      </svg>
    </span>
  );
}

function UserMark() {
  return (
    <span className="ai-avatar me" aria-hidden>
      <svg viewBox="0 0 24 24" overflow="visible">
        <path
          fill="currentColor"
          d="M12 5.2a3.3 3.3 0 1 1 0 6.6 3.3 3.3 0 0 1 0-6.6Zm0 8.4c3.5 0 6.6 1.7 6.6 4.1v.9H5.4v-.9c0-2.4 3.1-4.1 6.6-4.1Z"
        />
      </svg>
    </span>
  );
}

const AI_DOCK_MIN = 320;
const AI_DOCK_MAX_RATIO = 4 / 10;

function CopyMark({ copied }: { copied: boolean }) {
  return copied ? (
    <svg viewBox="0 0 24 24" overflow="visible" aria-hidden>
      <path
        fill="currentColor"
        d="M9.4 16.2 5.6 12.4l1.2-1.2 2.6 2.6 7.4-7.4 1.2 1.2z"
      />
    </svg>
  ) : (
    <svg viewBox="0 0 24 24" overflow="visible" aria-hidden>
      <path
        fill="currentColor"
        d="M15.5 4.5h-7A1.5 1.5 0 0 0 7 6v1H6a1.5 1.5 0 0 0-1.5 1.5v9A1.5 1.5 0 0 0 6 19h7a1.5 1.5 0 0 0 1.5-1.5V16H16a1.5 1.5 0 0 0 1.5-1.5v-9A1.5 1.5 0 0 0 16 4.5h-.5ZM13 17.5H6.5v-8H7V16A1.5 1.5 0 0 0 8.5 17.5H13Zm3.5-3h-7v-9h7Z"
      />
    </svg>
  );
}

function CopyBtn({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  async function copy(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const body = text ?? "";
    try {
      await navigator.clipboard.writeText(body);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = body;
      ta.setAttribute("readonly", "");
      ta.style.position = "fixed";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }
  return (
    <button type="button" className={`ai-copy${copied ? " ok" : ""}`} title={copied ? "已复制" : "复制"} onClick={(e) => void copy(e)}>
      <CopyMark copied={copied} />
    </button>
  );
}

export default function AiPanel({ sessionId, width, onWidth, visible = true, clusterNodes }: Props) {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [waiting, setWaiting] = useState(false);
  const [waitUser, setWaitUser] = useState(false);
  const [status, setStatus] = useState("");
  const boxRef = useRef<HTMLDivElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const dockRef = useRef<HTMLElement>(null);
  const dragRef = useRef<{ x: number; w: number } | null>(null);
  const widthRef = useRef(width);
  widthRef.current = width;
  const probed = useRef<string | null>(null);

  function fitComposer() {
    const el = taRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.overflowY = "hidden";
    const max = 132;
    if (el.scrollHeight > max) {
      el.style.height = `${max}px`;
      el.style.overflowY = "auto";
    } else {
      el.style.height = `${el.scrollHeight}px`;
    }
  }

  useEffect(() => {
    fitComposer();
  }, [input]);

  useEffect(() => {
    const focusComposer = () => {
      const el = taRef.current;
      if (!el || el.disabled) return;
      el.focus({ preventScroll: true });
    };
    return FocusOwner.registerAi(focusComposer);
  }, []);

  useEffect(() => {
    function guard(e: FocusEvent) {
      if (!FocusOwner.is("ai")) return;
      const t = e.target;
      if (!(t instanceof Element)) return;
      if (t.closest(".ai-dock")) return;
      // 终端 IME / 其它控件在 AI 持有期间抢焦 → 立刻夺回
      if (t.classList.contains("ime") || t.closest(".term-wrap")) {
        FocusOwner.focusAi();
      }
    }
    function pinAi(e: Event) {
      if (!FocusOwner.is("ai")) return;
      const t = e.target;
      if (t instanceof Element && t.closest(".ai-composer textarea, .ai-composer")) {
        FocusOwner.focusAi();
      }
    }
    window.addEventListener("focusin", guard, true);
    window.addEventListener("pointerup", pinAi, true);
    window.addEventListener("mouseup", pinAi, true);
    return () => {
      window.removeEventListener("focusin", guard, true);
      window.removeEventListener("pointerup", pinAi, true);
      window.removeEventListener("mouseup", pinAi, true);
    };
  }, []);

  useEffect(() => {
    boxRef.current?.scrollTo({ top: boxRef.current.scrollHeight });
  }, [msgs, status, waiting]);

  useEffect(() => {
    if (probed.current === sessionId) return;
    probed.current = sessionId;
    setMsgs([]);
    if (sessionId.startsWith("cluster:")) {
      setWaiting(false);
      setStatus("");
      setMsgs([{ role: "system", text: "集群助手已就绪。指令会自动抓取已连接节点的日志并做综合分析。" }]);
      return;
    }
    setWaiting(true);
    setStatus("正在异步加载主机信息…");
    void api
      .aiPrepare(sessionId)
      .then((facts) => {
        setWaiting(false);
        setStatus("");
        if (facts.trim()) {
          setMsgs([{ role: "system", text: `已采集当前服务器信息：\n${facts}` }]);
        } else {
          setMsgs([{ role: "system", text: "未能采集主机信息，仍可提问；建议先检查连接。" }]);
        }
      })
      .catch((e) => {
        setWaiting(false);
        setStatus("");
        setMsgs([{ role: "system", text: `采集主机信息失败：${e}` }]);
      });
  }, [sessionId]);

  useEffect(() => {
    let stop = false;
    const un = listen<AiEvent>("ai-event", (e) => {
      if (stop || e.payload.session_id !== sessionId) return;
      const p = e.payload;
      if (p.kind === "waiting") {
        setWaiting(true);
        setWaitUser(false);
      }
      if (p.kind === "danger") {
        setWaiting(true);
        setWaitUser(true);
        setStatus("等待你确认危险命令…");
      }
      if (p.kind === "wait-user") {
        setWaiting(true);
        setWaitUser(true);
        setStatus(p.text || "等待你完成本地操作…");
      }
      if (p.kind === "waiting-end" || p.kind === "done") {
        setWaiting(false);
        setWaitUser(false);
      }
      if (p.kind === "status") setStatus(p.text);
      if (p.kind === "done") setStatus(p.text === "本轮结束" ? "" : p.text);
      if (p.kind === "message" || p.kind === "error") {
        const role = p.role === "user" ? "user" : p.role === "assistant" ? "assistant" : "system";
        setMsgs((m) => [...m, { role, text: p.text }]);
      }
    });
    return () => {
      stop = true;
      un.then((f) => f());
    };
  }, [sessionId]);

  useEffect(() => {
    if (!visible) return;
    const stage = dockRef.current?.parentElement;
    if (!stage) return;
    const apply = () => {
      const total = stage.clientWidth;
      const max = Math.max(AI_DOCK_MIN, Math.floor(total * AI_DOCK_MAX_RATIO));
      const w = widthRef.current;
      const next = Math.min(max, Math.max(AI_DOCK_MIN, w));
      if (next !== w) onWidth(next);
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(stage);
    return () => ro.disconnect();
  }, [onWidth, visible]);

  function clampWidth(next: number) {
    const stage = dockRef.current?.parentElement;
    const total = stage?.clientWidth || 0;
    const max = Math.max(AI_DOCK_MIN, Math.floor(total * AI_DOCK_MAX_RATIO));
    return Math.min(max, Math.max(AI_DOCK_MIN, Math.round(next)));
  }

  function onSplitPointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = { x: e.clientX, w: width };
    document.body.classList.add("col-resizing");
  }

  function onSplitPointerMove(e: React.PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current;
    if (!drag || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
    onWidth(clampWidth(drag.w + drag.x - e.clientX));
  }

  function onSplitPointerUp(e: React.PointerEvent<HTMLDivElement>) {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    dragRef.current = null;
    document.body.classList.remove("col-resizing");
  }

  async function send() {
    const text = input.trim();
    if (!text || (waiting && !waitUser)) return;
    setInput("");
    requestAnimationFrame(fitComposer);
    if (!waitUser) {
      setWaiting(true);
      setStatus("正在思考…");
    }
    try {
      await (clusterNodes?.length
        ? api.aiClusterAsk(sessionId, text, clusterNodes)
        : api.aiAsk(sessionId, text));
    } catch (e) {
      setWaiting(false);
      setWaitUser(false);
      setStatus("");
      setMsgs((m) => [...m, { role: "system", text: String(e) }]);
    }
  }

  return (
    <aside className="ai-dock" ref={dockRef} style={{ width }} hidden={!visible}>
      <div
        className="ai-split"
        onPointerDown={onSplitPointerDown}
        onPointerMove={onSplitPointerMove}
        onPointerUp={onSplitPointerUp}
        onPointerCancel={onSplitPointerUp}
      />
      <div className="ai-dock-head">
        <strong>AI助手</strong>
        {waitUser && (
          <button type="button" className="ai-stop" onClick={() => void api.aiResume(sessionId)}>
            继续
          </button>
        )}
        {waiting && (
          <button type="button" className="ai-stop" onClick={() => void api.aiCancel(sessionId)}>
            打断
          </button>
        )}
      </div>
      <div className="ai-history" ref={boxRef}>
        {msgs.map((m, i) => (
          <div key={i} className={`ai-row ${m.role}`}>
            {m.role !== "user" ? <AiMark /> : null}
            <div className="ai-bubble-wrap">
              <div className={`ai-bubble ${m.role}`}>{m.text}</div>
              <CopyBtn text={m.text} />
            </div>
            {m.role === "user" ? <UserMark /> : null}
          </div>
        ))}
        {status && <div className="ai-status">{status}</div>}
      </div>
      <form
        className="ai-input"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <div className="ai-composer">
          <textarea
            ref={taRef}
            value={input}
            disabled={waiting && !waitUser}
            placeholder={
              waitUser ? "完成后发消息、点「继续」，或等终端回到提示符" : waiting ? "等待终端命令完成…" : "例如：配置 nginx 反向代理"
            }
            rows={1}
            onMouseDown={(e) => {
              e.stopPropagation();
              if (waiting && !waitUser) return;
              FocusOwner.takeAi();
            }}
            onPointerDown={(e) => {
              e.stopPropagation();
              if (waiting && !waitUser) return;
              FocusOwner.takeAi();
            }}
            onMouseUp={(e) => {
              e.stopPropagation();
              if (waiting && !waitUser) return;
              FocusOwner.takeAi();
            }}
            onPointerUp={(e) => {
              e.stopPropagation();
              if (waiting && !waitUser) return;
              FocusOwner.takeAi();
            }}
            onFocus={() => {
              if (waiting && !waitUser) return;
              FocusOwner.claim("ai");
            }}
            onBlur={() => {
              window.setTimeout(() => {
                if (!FocusOwner.is("ai")) return;
                const cur = document.activeElement;
                if (cur instanceof Element && cur.closest(".ai-composer textarea")) return;
                // 只对抗终端抢焦，不对抗用户点到别处
                if (cur instanceof Element && (cur.classList.contains("ime") || cur.closest(".term-wrap"))) {
                  FocusOwner.focusAi();
                }
              }, 0);
            }}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button
            type="submit"
            className="ai-send"
            title="发送"
            aria-label="发送"
            tabIndex={-1}
            disabled={(waiting && !waitUser) || !input.trim()}
            onMouseDown={(e) => {
              // 防止按钮在 mouseup 时抢走 textarea 焦点
              e.preventDefault();
            }}
          >
            <svg viewBox="0 0 24 24" overflow="visible" aria-hidden>
              <path
                fill="currentColor"
                d="M4.2 11.1 18.6 4.8a.9.9 0 0 1 1.2 1.1L13.4 19.4a.9.9 0 0 1-1.6.1l-2.5-4.8-4.8-2.4a.9.9 0 0 1-.3-1.2Z"
              />
            </svg>
          </button>
        </div>
      </form>
    </aside>
  );
}
