import { useEffect, useState } from "react";

const EVENT = "ferrassh-toast";

export function showToast(text: string) {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: text }));
}

export default function ToastHost() {
  const [text, setText] = useState("");
  const [on, setOn] = useState(false);

  useEffect(() => {
    let hide = 0;
    const onEv = (e: Event) => {
      const msg = String((e as CustomEvent).detail || "").trim();
      if (!msg) return;
      setText(msg);
      setOn(true);
      window.clearTimeout(hide);
      hide = window.setTimeout(() => setOn(false), 1400);
    };
    window.addEventListener(EVENT, onEv);
    return () => {
      window.removeEventListener(EVENT, onEv);
      window.clearTimeout(hide);
    };
  }, []);

  return (
    <div className={`lite-toast${on ? " on" : ""}`} role="status">
      {text}
    </div>
  );
}
