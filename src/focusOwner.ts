/** 全局唯一焦点所有者：同一时刻只有一个输入面可持有光标。 */

export type FocusOwnerKind = "term" | "ai" | "other";

type Focuser = () => void;

let owner: FocusOwnerKind = "term";
let termFocuser: Focuser | null = null;
let aiFocuser: Focuser | null = null;
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((fn) => {
    try {
      fn();
    } catch {
      /* ignore */
    }
  });
}

export const FocusOwner = {
  get(): FocusOwnerKind {
    return owner;
  },

  is(kind: FocusOwnerKind) {
    return owner === kind;
  },

  claim(kind: FocusOwnerKind) {
    if (owner === kind) return;
    owner = kind;
    notify();
  },

  registerTerm(fn: Focuser) {
    termFocuser = fn;
    return () => {
      if (termFocuser === fn) termFocuser = null;
    };
  },

  registerAi(fn: Focuser) {
    aiFocuser = fn;
    return () => {
      if (aiFocuser === fn) aiFocuser = null;
    };
  },

  /** 仅当所有者已是 term 时聚焦，不会强行抢权。 */
  focusTerm() {
    if (owner !== "term") return;
    termFocuser?.();
  },

  /** 仅当所有者已是 ai 时聚焦。 */
  focusAi() {
    if (owner !== "ai") return;
    aiFocuser?.();
  },

  /** 主动要终端光标（点终端时用）。 */
  takeTerm() {
    owner = "term";
    notify();
    termFocuser?.();
  },

  /** 主动要 AI 光标（点 AI 输入框 / 松开鼠标时钉住）。 */
  takeAi() {
    owner = "ai";
    notify();
    aiFocuser?.();
  },

  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  },
};
