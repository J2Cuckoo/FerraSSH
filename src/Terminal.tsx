import { useCallback, useEffect, useRef, useState } from "react";
import { isDefaultTermFg } from "./termTheme";
import type { TermCell, TermFrame } from "./types";
import { api } from "./api";
import ContextMenu from "./ContextMenu";
import { showToast } from "./toast";
import { FocusOwner } from "./focusOwner";

type Props = {
  sessionId: string;
  frame: TermFrame | null;
  fontFamily: string;
  fontSize: number;
  defaultFg: number;
  onFrame: (frame: TermFrame) => void;
  visible?: boolean;
  refreshNonce?: number;
};

type CellPos = { y: number; i: number };
type Sel = { a: CellPos; b: CellPos };

const PAD_X = 8;
const SCROLL_W = 12;

function hex(n: number): string {
  return `#${n.toString(16).padStart(6, "0")}`;
}

const FLAG_INVERSE = 0b0000_0000_0000_0001;
const FLAG_BOLD = 0b0000_0000_0000_0010;
const FLAG_ITALIC = 0b0000_0000_0000_0100;
const FLAG_WIDE_CHAR = 0b0000_0000_0010_0000;

const CJK_FALLBACK = '"Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "Noto Sans Mono CJK SC"';

function canvasFont(px: number, family: string, style = ""): string {
  const quoted = family
    .split(",")
    .map((s) => s.trim().replace(/^["']|["']$/g, ""))
    .filter(Boolean)
    .map((s) => (/^(monospace|sans-serif|serif|cursive|fantasy)$/i.test(s) ? s : `"${s}"`))
    .join(", ");
  const prefix = style ? `${style} ` : "";
  return `${prefix}${px}px ${quoted}, ${CJK_FALLBACK}, monospace`;
}

function cellWOf(c: TermCell, cellW: number) {
  return (c.flags & FLAG_WIDE_CHAR) !== 0 ? cellW * 2 : cellW;
}

function cmpPos(a: CellPos, b: CellPos) {
  return a.y === b.y ? a.i - b.i : a.y - b.y;
}

function ordered(sel: Sel): [CellPos, CellPos] {
  return cmpPos(sel.a, sel.b) <= 0 ? [sel.a, sel.b] : [sel.b, sel.a];
}

function cellSelected(y: number, i: number, sel: Sel | null) {
  if (!sel) return false;
  const [s, e] = ordered(sel);
  if (y < s.y || y > e.y) return false;
  if (s.y === e.y) return i >= s.i && i <= e.i;
  if (y === s.y) return i >= s.i;
  if (y === e.y) return i <= e.i;
  return true;
}

function lineAt(frame: TermFrame, y: number) {
  return frame.lines.find((l) => l.y === y) ?? frame.lines[y];
}

function hitPos(frame: TermFrame, px: number, py: number, cellW: number, cellH: number): { pos: CellPos; blank: boolean } {
  const y = Math.max(0, Math.min(Math.max(0, frame.rows - 1), Math.floor(py / cellH)));
  const line = lineAt(frame, y);
  if (!line || line.cells.length === 0) return { pos: { y, i: 0 }, blank: true };
  let x = 0;
  for (let i = 0; i < line.cells.length; i++) {
    const w = cellWOf(line.cells[i], cellW);
    if (px < x + w) {
      const ch = cellChar(line.cells[i]);
      return { pos: { y, i }, blank: isBlankChar(ch) };
    }
    x += w;
  }
  return { pos: { y, i: line.cells.length - 1 }, blank: true };
}

function selectedText(frame: TermFrame, sel: Sel | null) {
  if (!sel) return "";
  const [s, e] = ordered(sel);
  const rows: string[] = [];
  for (let y = s.y; y <= e.y; y++) {
    const line = lineAt(frame, y);
    if (!line) {
      rows.push("");
      continue;
    }
    const from = y === s.y ? s.i : 0;
    const to = y === e.y ? e.i : line.cells.length - 1;
    let t = "";
    for (let i = Math.max(0, from); i <= Math.min(to, line.cells.length - 1); i++) {
      t += line.cells[i].ch || " ";
    }
    rows.push(t.replace(/ +$/g, ""));
  }
  return rows.join("\n");
}

function samePos(a: CellPos, b: CellPos) {
  return a.y === b.y && a.i === b.i;
}

function cellChar(c: TermCell | undefined) {
  return c?.ch || " ";
}

function isBlankChar(ch: string) {
  return ch === " " || ch === "\t" || ch === "\u00a0";
}

function isTokenChar(ch: string) {
  if (isBlankChar(ch) || !ch) return false;
  return !/^[()[\]{}<>|;&'"`]$/.test(ch);
}

function wordRange(frame: TermFrame, pos: CellPos): Sel | null {
  const line = lineAt(frame, pos.y);
  if (!line?.cells.length) return null;
  const ch = cellChar(line.cells[pos.i]);
  if (!isTokenChar(ch)) return null;
  let a = pos.i;
  let b = pos.i;
  while (a > 0 && isTokenChar(cellChar(line.cells[a - 1]))) a -= 1;
  while (b + 1 < line.cells.length && isTokenChar(cellChar(line.cells[b + 1]))) b += 1;
  return { a: { y: pos.y, i: a }, b: { y: pos.y, i: b } };
}

function termScrollBar(frame: TermFrame | null, trackH: number) {
  const rows = Math.max(1, frame?.rows || 1);
  const max = Math.max(0, frame?.scroll_max || 0);
  const offset = Math.min(max, Math.max(0, frame?.scroll_offset || 0));
  const total = max + rows;
  const thumbH = Math.max(28, Math.round((rows / Math.max(total, 1)) * trackH));
  const travel = Math.max(1, trackH - thumbH);
  const top = max <= 0 ? travel : Math.round(((max - offset) / max) * travel);
  return { max, offset, thumbH, top, travel, show: max > 0 };
}

function offsetOf(frame: TermFrame | null | undefined) {
  return frame?.scroll_offset || 0;
}

function absFromView(pos: CellPos, offset: number): CellPos {
  return { y: pos.y - offset, i: pos.i };
}

function absToView(sel: Sel, offset: number): Sel {
  return {
    a: { y: sel.a.y + offset, i: sel.a.i },
    b: { y: sel.b.y + offset, i: sel.b.i },
  };
}

function offsetFromThumbTop(y: number, travel: number, max: number) {
  const ratio = Math.min(1, Math.max(0, y / Math.max(travel, 1)));
  return Math.round((1 - ratio) * max);
}

type TermKeyEvent = {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  preventDefault: () => void;
};

export default function TerminalView({ sessionId, frame, fontFamily, fontSize, defaultFg, onFrame, visible = true, refreshNonce = 0 }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const imeRef = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const focusTimers = useRef<number[]>([]);
  const focusRafs = useRef<number[]>([]);
  const focusGen = useRef(0);
  const [focusMine, setFocusMine] = useState(() => FocusOwner.is("term"));
  const frameRef = useRef(frame);
  frameRef.current = frame;
  const onFrameRef = useRef(onFrame);
  onFrameRef.current = onFrame;
  const selRef = useRef<Sel | null>(null);
  const dragRef = useRef<{ start: CellPos; x: number; y: number; moved: boolean } | null>(null);
  const barDrag = useRef<{ grab: number; travel: number; max: number } | null>(null);
  const barPending = useRef<number | null>(null);
  const barRaf = useRef(0);
  const lastPtr = useRef({ x: 0, y: 0 });
  const autoDir = useRef(0);
  const autoStep = useRef(1);
  const autoTimer = useRef(0);
  const autoBusy = useRef(false);
  const [sel, setSel] = useState<Sel | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [barH, setBarH] = useState(0);
  selRef.current = sel;

  const cell = Math.max(11, fontSize);
  const cellW = Math.ceil(cell * 0.62);
  const cellH = Math.ceil(cell * 1.35);

  const paint = useCallback(
    (f: TermFrame, cursorOn = true) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const dpr = window.devicePixelRatio || 1;
      const width = f.cols * cellW;
      const height = f.rows * cellH;
      canvas.width = Math.floor(width * dpr);
      canvas.height = Math.floor(height * dpr);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.fillStyle = "#0b0f14";
      ctx.fillRect(0, 0, width, height);
      ctx.font = canvasFont(cell, fontFamily);
      ctx.textBaseline = "top";
      const curSel = selRef.current ? absToView(selRef.current, offsetOf(f)) : null;

      for (const line of f.lines) {
        let x = 0;
        let i = 0;
        for (const c of line.cells) {
          const inverse = (c.flags & FLAG_INVERSE) !== 0;
          let fg = inverse ? c.bg : c.fg;
          let bg = inverse ? c.fg : c.bg;
          if (isDefaultTermFg(fg)) fg = defaultFg;
          if (isDefaultTermFg(bg) && bg) bg = defaultFg;
          const w = cellWOf(c, cellW);
          const on = cellSelected(line.y, i, curSel);
          ctx.fillStyle = hex(bg || 0x0b0f14);
          ctx.fillRect(x, line.y * cellH, w, cellH);
          if (on) {
            ctx.fillStyle = "rgba(61,205,195,0.32)";
            ctx.fillRect(x, line.y * cellH, w, cellH);
          }
          if (c.ch && c.ch !== " ") {
            ctx.fillStyle = hex(fg || defaultFg);
            if (c.flags & FLAG_BOLD) ctx.font = canvasFont(cell, fontFamily, "bold");
            else if (c.flags & FLAG_ITALIC) ctx.font = canvasFont(cell, fontFamily, "italic");
            else ctx.font = canvasFont(cell, fontFamily);
            ctx.fillText(c.ch, x, line.y * cellH + 2);
          }
          x += w;
          i += 1;
        }
      }

      if (f.cursor_visible && cursorOn && !curSel && FocusOwner.is("term")) {
        ctx.fillStyle = "rgba(61,205,195,0.95)";
        ctx.fillRect(f.cursor_x * cellW, f.cursor_y * cellH + 1, Math.max(2, Math.round(cellW * 0.12)), cellH - 2);
      }
    },
    [cell, cellH, cellW, defaultFg, fontFamily],
  );

  const blinkOn = useRef(true);

  useEffect(() => {
    blinkOn.current = true;
    if (frame) paint(frame, true);
  }, [frame, paint, sel, focusMine]);

  useEffect(() => {
    const id = window.setInterval(() => {
      blinkOn.current = !blinkOn.current;
      const f = frameRef.current;
      if (f) paint(f, blinkOn.current);
    }, 530);
    return () => window.clearInterval(id);
  }, [paint, focusMine]);

  useEffect(() => {
    const el = canvasRef.current?.parentElement;
    if (!el) return;
    let raf = 0;
    let lastCols = 0;
    let lastRows = 0;
    const sendSize = (force = false) => {
      if (raf) return;
      raf = window.requestAnimationFrame(() => {
        raf = 0;
        if (el.clientWidth < 16 || el.clientHeight < 16) return;
        setBarH(el.clientHeight);
        const padBottom = cellH;
        const cols = Math.max(2, Math.floor((el.clientWidth - PAD_X - SCROLL_W) / cellW));
        const rows = Math.max(1, Math.floor((el.clientHeight - padBottom) / cellH));
        if (!force && cols === lastCols && rows === lastRows) return;
        lastCols = cols;
        lastRows = rows;
        api.termResize(sessionId, cols, rows).then((next) => onFrameRef.current(next)).catch(() => {});
      });
    };
    sendSize();
    const ro = new ResizeObserver(() => sendSize());
    ro.observe(el);
    return () => {
      if (raf) window.cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [sessionId, cellW, cellH, visible, refreshNonce]);

  useEffect(() => {
    if (!visible) return;
    api.termFrame(sessionId).then((next) => onFrameRef.current(next)).catch(() => {});
  }, [visible, sessionId, refreshNonce]);

  function clearFocusTimers() {
    for (const id of focusTimers.current) window.clearTimeout(id);
    focusTimers.current = [];
    for (const id of focusRafs.current) window.cancelAnimationFrame(id);
    focusRafs.current = [];
  }

  function applyTermFocus() {
    if (!visibleRef.current) return;
    if (!FocusOwner.is("term")) return;
    composing.current = false;
    const ime = imeRef.current;
    if (!ime) return;
    wrapRef.current?.focus({ preventScroll: true });
    ime.value = "";
    ime.focus({ preventScroll: true });
  }

  function focusTerm() {
    FocusOwner.takeTerm();
    const gen = ++focusGen.current;
    clearFocusTimers();
    const run = () => {
      if (gen !== focusGen.current) return;
      FocusOwner.focusTerm();
    };
    focusTimers.current.push(window.setTimeout(run, 0));
    focusRafs.current.push(window.requestAnimationFrame(run));
  }

  useEffect(() => {
    const unsub = FocusOwner.subscribe(() => {
      setFocusMine(FocusOwner.is("term"));
      if (!FocusOwner.is("term")) {
        focusGen.current += 1;
        clearFocusTimers();
      }
    });
    const unreg = FocusOwner.registerTerm(applyTermFocus);
    return () => {
      unsub();
      unreg();
      clearFocusTimers();
    };
  }, []);

  useEffect(() => {
    if (!visible) return;
    if (!FocusOwner.is("term")) return;
    const gen = ++focusGen.current;
    const id = window.requestAnimationFrame(() => {
      if (gen !== focusGen.current) return;
      FocusOwner.focusTerm();
    });
    return () => window.cancelAnimationFrame(id);
  }, [visible, sessionId, refreshNonce]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  useEffect(() => {
    function onDown(e: MouseEvent) {
      const t = e.target;
      if (!(t instanceof Element)) return;
      const wrap = wrapRef.current;
      if (t.closest(".ai-dock")) {
        // AI 面板自己 claim("ai")；这里只作废终端延迟抢焦
        focusGen.current += 1;
        clearFocusTimers();
        return;
      }
      if (wrap && wrap.contains(t)) {
        FocusOwner.claim("term");
      } else if (t.closest("input, textarea, select, [contenteditable=true], .modal, .page-overlay")) {
        FocusOwner.claim("other");
        focusGen.current += 1;
        clearFocusTimers();
      }
    }
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, []);

  useEffect(() => {
    return () => {
      if (autoTimer.current) window.clearInterval(autoTimer.current);
      autoTimer.current = 0;
    };
  }, []);

  function encodeKey(e: TermKeyEvent, appCursor: boolean): string | number[] | null {
    if (e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1) {
      const k = e.key.toLowerCase();
      if (k >= "a" && k <= "z") return [k.charCodeAt(0) - 96];
    }
    switch (e.key) {
      case "Enter":
        return "\r";
      case "Backspace":
        return "\x7f";
      case "Tab":
        return e.shiftKey ? "\x1b[Z" : "\t";
      case "Escape":
        return "\x1b";
      case "ArrowUp":
        return appCursor ? "\x1bOA" : "\x1b[A";
      case "ArrowDown":
        return appCursor ? "\x1bOB" : "\x1b[B";
      case "ArrowRight":
        return appCursor ? "\x1bOC" : "\x1b[C";
      case "ArrowLeft":
        return appCursor ? "\x1bOD" : "\x1b[D";
      case "Home":
        return "\x1b[H";
      case "End":
        return "\x1b[F";
      case "PageUp":
        return "\x1b[5~";
      case "PageDown":
        return "\x1b[6~";
      case "Delete":
        return "\x1b[3~";
      case "Insert":
        return "\x1b[2~";
      default:
        if (e.key.startsWith("F")) {
          const n = Number(e.key.slice(1));
          const map: Record<number, string> = {
            1: "\x1bOP",
            2: "\x1bOQ",
            3: "\x1bOR",
            4: "\x1bOS",
            5: "\x1b[15~",
            6: "\x1b[17~",
            7: "\x1b[18~",
            8: "\x1b[19~",
            9: "\x1b[20~",
            10: "\x1b[21~",
            11: "\x1b[23~",
            12: "\x1b[24~",
          };
          return map[n] ?? null;
        }
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) return e.key;
        return null;
    }
  }

  async function send(payload: string | number[]) {
    if (typeof payload === "string") await api.termWriteText(sessionId, payload);
    else await api.termWrite(sessionId, payload);
  }

  async function pasteText(text: string) {
    const cleaned = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").replace(/\n+$/g, "");
    if (!cleaned) return;
    const f = frameRef.current;
    const payload = f?.bracketed_paste ? `\x1b[200~${cleaned}\x1b[201~` : cleaned.replace(/\n/g, "\r");
    await send(payload);
    setSel(null);
    focusTerm();
  }

  function toWindowsClipboard(text: string, notify = false) {
    if (!text) return;
    void api.clipboardWrite(text).then(() => {
      if (notify) {
        setSel(null);
        showToast("复制成功");
      }
    }).catch(() => {});
  }

  function copySelection() {
    const cur = selRef.current;
    const f = frameRef.current;
    if (!cur || !f) return;
    const [s, e] = ordered(cur);
    const visible = selectedText(f, absToView(cur, offsetOf(f)));
    api.termRangeText(sessionId, s.y, s.i, e.y, e.i)
      .then((text) => toWindowsClipboard(text || visible, true))
      .catch(() => toWindowsClipboard(visible, true));
  }

  async function pasteFromWindows() {
    try {
      const text = await api.clipboardRead();
      await pasteText(text);
    } catch {
      focusTerm();
    }
  }

  function screenText(f: TermFrame) {
    return f.lines
      .slice()
      .sort((a, b) => a.y - b.y)
      .map((line) => line.cells.map((c) => c.ch || " ").join("").replace(/ +$/g, ""))
      .join("\n");
  }

  function posFromEvent(e: { clientX: number; clientY: number }, f = frameRef.current): { pos: CellPos; blank: boolean } | null {
    const canvas = canvasRef.current;
    if (!canvas || !f) return null;
    const r = canvas.getBoundingClientRect();
    return hitPos(f, e.clientX - r.left, e.clientY - r.top, cellW, cellH);
  }

  function extendSelToPointer(clientX: number, clientY: number, frame?: TermFrame) {
    const drag = dragRef.current;
    const f = frame || frameRef.current;
    if (!drag || !f) return;
    const hit = posFromEvent({ clientX, clientY }, f);
    if (!hit) return;
    const b = absFromView(hit.pos, offsetOf(f));
    if (!selRef.current || !samePos(selRef.current.b, b)) {
      setSel({ a: drag.start, b });
    }
  }

  function stopAutoScroll() {
    autoDir.current = 0;
    autoBusy.current = false;
    if (autoTimer.current) {
      window.clearInterval(autoTimer.current);
      autoTimer.current = 0;
    }
  }

  function autoScrollTick() {
    if (autoBusy.current || !dragRef.current) return;
    const dir = autoDir.current;
    if (!dir) {
      stopAutoScroll();
      return;
    }
    const f = frameRef.current;
    if (!f) return;
    const max = f.scroll_max || 0;
    if (max <= 0) {
      stopAutoScroll();
      return;
    }
    const next = Math.min(max, Math.max(0, offsetOf(f) + dir * autoStep.current));
    if (next === offsetOf(f)) return;
    autoBusy.current = true;
    api.termScrollTo(sessionId, next)
      .then((frame) => {
        onFrameRef.current(frame);
        extendSelToPointer(lastPtr.current.x, lastPtr.current.y, frame);
      })
      .catch(() => {})
      .finally(() => {
        autoBusy.current = false;
      });
  }

  function updateAutoScroll(clientX: number, clientY: number) {
    lastPtr.current = { x: clientX, y: clientY };
    const canvas = canvasRef.current;
    const f = frameRef.current;
    if (!canvas || !dragRef.current || !f || !(f.scroll_max || 0)) {
      stopAutoScroll();
      return;
    }
    const r = canvas.getBoundingClientRect();
    const wrap = wrapRef.current?.getBoundingClientRect();
    const top = wrap ? wrap.top : r.top;
    const bottom = wrap ? wrap.bottom : r.bottom;
    const edge = Math.max(8, cellH);
    let dir = 0;
    let step = 1;
    if (clientY <= top + edge) {
      dir = 1;
      step = clientY < top ? Math.min(4, 1 + Math.floor((top - clientY) / 28)) : 1;
    } else if (clientY >= bottom - edge) {
      dir = -1;
      step = clientY > bottom ? Math.min(4, 1 + Math.floor((clientY - bottom) / 28)) : 1;
    }
    autoStep.current = step;
    if (dir === 0) {
      stopAutoScroll();
      return;
    }
    autoDir.current = dir;
    if (!autoTimer.current) {
      autoScrollTick();
      autoTimer.current = window.setInterval(autoScrollTick, 40);
    }
  }

  function onCanvasPointerDown(e: React.PointerEvent<HTMLCanvasElement>) {
    if (e.button !== 0) return;
    FocusOwner.takeTerm();
    setMenu(null);
    stopAutoScroll();
    const f = frameRef.current;
    const hit = posFromEvent(e, f);
    if (!hit || !f) return;
    const start = absFromView(hit.pos, offsetOf(f));
    lastPtr.current = { x: e.clientX, y: e.clientY };
    dragRef.current = { start, x: e.clientX, y: e.clientY, moved: false };
    setSel({ a: start, b: start });
    e.currentTarget.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onCanvasPointerMove(e: React.PointerEvent<HTMLCanvasElement>) {
    const drag = dragRef.current;
    if (!drag || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
    if (Math.abs(e.clientX - drag.x) + Math.abs(e.clientY - drag.y) > 3) drag.moved = true;
    lastPtr.current = { x: e.clientX, y: e.clientY };
    extendSelToPointer(e.clientX, e.clientY);
    updateAutoScroll(e.clientX, e.clientY);
  }

  function onCanvasPointerUp(e: React.PointerEvent<HTMLCanvasElement>) {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    stopAutoScroll();
    const drag = dragRef.current;
    dragRef.current = null;
    if (drag && !drag.moved) setSel(null);
    // 仅在仍拥有终端焦点时恢复 IME；若用户已点到 AI，绝不能在 mouseup 抢回
    if (FocusOwner.is("term")) FocusOwner.focusTerm();
  }

  function onCanvasDoubleClick(e: React.MouseEvent<HTMLCanvasElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    setMenu(null);
    const f = frameRef.current;
    const hit = posFromEvent(e, f);
    if (!f || !hit || hit.blank) {
      setSel(null);
      return;
    }
    const next = wordRange(f, hit.pos);
    if (!next) {
      setSel(null);
      return;
    }
    const offset = offsetOf(f);
    setSel({ a: absFromView(next.a, offset), b: absFromView(next.b, offset) });
  }

  const copied = !!(sel && (!samePos(sel.a, sel.b) || (frame && selectedText(frame, absToView(sel, offsetOf(frame))))));
  const bar = termScrollBar(frame, barH);

  function applyScrollTo(offset: number) {
    barPending.current = offset;
    if (barRaf.current) return;
    barRaf.current = window.requestAnimationFrame(() => {
      barRaf.current = 0;
      const next = barPending.current;
      barPending.current = null;
      if (next == null) return;
      api.termScrollTo(sessionId, next).then(onFrame).catch(() => {});
    });
  }

  function onBarPointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0 || !bar.show) return;
    e.preventDefault();
    e.stopPropagation();
    const track = e.currentTarget.getBoundingClientRect();
    const y = e.clientY - track.top;
    const onThumb = y >= bar.top && y <= bar.top + bar.thumbH;
    if (onThumb) {
      barDrag.current = { grab: y - bar.top, travel: bar.travel, max: bar.max };
    } else {
      applyScrollTo(offsetFromThumbTop(y - bar.thumbH / 2, bar.travel, bar.max));
      barDrag.current = { grab: bar.thumbH / 2, travel: bar.travel, max: bar.max };
    }
    e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onBarPointerMove(e: React.PointerEvent<HTMLDivElement>) {
    const drag = barDrag.current;
    if (!drag || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
    e.preventDefault();
    const track = e.currentTarget.getBoundingClientRect();
    applyScrollTo(offsetFromThumbTop(e.clientY - track.top - drag.grab, drag.travel, drag.max));
  }

  function onBarPointerUp(e: React.PointerEvent<HTMLDivElement>) {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    barDrag.current = null;
  }

  function onTermKeyDown(e: TermKeyEvent) {
    if (composing.current) return;
    const key = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && !e.altKey && key === "c") {
      const cur = selRef.current;
      if (cur && !samePos(cur.a, cur.b)) {
        e.preventDefault();
        copySelection();
        return;
      }
      if (e.shiftKey) {
        e.preventDefault();
        const f = frameRef.current;
        toWindowsClipboard(f ? screenText(f) : "", true);
        return;
      }
    }
    if (((e.ctrlKey || e.metaKey) && !e.altKey && key === "v") || (e.shiftKey && e.key === "Insert")) {
      e.preventDefault();
      void pasteFromWindows();
      return;
    }
    const f = frameRef.current;
    const seq = encodeKey(e, !!f?.app_cursor);
    if (seq != null) {
      e.preventDefault();
      void send(seq);
    }
  }

  return (
    <div
      ref={wrapRef}
      className="term-wrap"
      tabIndex={-1}
      onClick={() => {
        if (!menu) focusTerm();
      }}
      onKeyDown={(e) => {
        if (e.target === imeRef.current) return;
        onTermKeyDown(e);
      }}
      onWheel={(e) => {
        e.preventDefault();
        const lines = Math.max(1, Math.round(Math.abs(e.deltaY) / 40));
        api.termScroll(sessionId, e.deltaY > 0 ? -lines : lines)
          .then((next) => {
            onFrame(next);
            if (dragRef.current) extendSelToPointer(e.clientX, e.clientY, next);
          })
          .catch(() => {});
      }}
    >
      <canvas
        ref={canvasRef}
        className="term-canvas"
        onPointerDown={onCanvasPointerDown}
        onPointerMove={onCanvasPointerMove}
        onPointerUp={onCanvasPointerUp}
        onPointerCancel={onCanvasPointerUp}
        onDoubleClick={onCanvasDoubleClick}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      />
      {bar.show && (
        <div
          className="term-scroll"
          onPointerDown={onBarPointerDown}
          onPointerMove={onBarPointerMove}
          onPointerUp={onBarPointerUp}
          onPointerCancel={onBarPointerUp}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          <div className="term-scroll-thumb" style={{ height: bar.thumbH, top: bar.top }} />
        </div>
      )}
      <textarea
        ref={imeRef}
        className="ime"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        onKeyDown={(e) => onTermKeyDown(e)}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={(e) => {
          composing.current = false;
          if (e.data) send(e.data);
          e.currentTarget.value = "";
        }}
        onPaste={(e) => {
          e.preventDefault();
          pasteText(e.clipboardData.getData("text"));
        }}
      />
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} className="term-ctx">
          <button
            type="button"
            disabled={!copied}
            onClick={() => {
              copySelection();
              setMenu(null);
              focusTerm();
            }}
          >
            复制
          </button>
          <button
            type="button"
            onClick={() => {
              void pasteFromWindows().finally(() => {
                setMenu(null);
                focusTerm();
              });
            }}
          >
            粘贴
          </button>
        </ContextMenu>
      )}
    </div>
  );
}
