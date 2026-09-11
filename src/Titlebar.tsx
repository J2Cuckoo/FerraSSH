import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Props = {
  settingsEnabled?: boolean;
  settingsActive?: boolean;
  aiActive?: boolean;
  aboutActive?: boolean;
  onHome?: () => void;
  onSettings?: () => void;
  onAi?: () => void;
  onAbout?: () => void;
  onUpdate?: () => void;
};

export default function Titlebar({
  settingsEnabled = true,
  settingsActive = false,
  aiActive = false,
  aboutActive = false,
  onHome,
  onSettings,
  onAi,
  onAbout,
  onUpdate,
}: Props) {
  const [maximized, setMaximized] = useState(false);
  const win = useMemo(() => getCurrentWindow(), []);

  useEffect(() => {
    let stop = false;
    const sync = async () => {
      try {
        const max = await win.isMaximized();
        if (!stop) setMaximized(max);
      } catch {
        /* permissions may be missing during boot */
      }
    };
    void sync();
    const un = [win.onResized(() => void sync()), win.onMoved(() => void sync())];
    return () => {
      stop = true;
      un.forEach((p) => p.then((f) => f()));
    };
  }, [win]);

  function isChromeControl(target: EventTarget | null) {
    return target instanceof Element && !!target.closest("button, a, input, select, textarea, label, .window-controls");
  }

  function onMouseDown(e: React.MouseEvent<HTMLElement>) {
    if (e.button !== 0 || isChromeControl(e.target)) return;
    void win.startDragging();
  }

  function onDoubleClick(e: React.MouseEvent<HTMLElement>) {
    if (isChromeControl(e.target)) return;
    void toggleMaximize();
  }

  async function toggleMaximize() {
    try {
      if (await win.isMaximized()) await win.unmaximize();
      else await win.maximize();
      setMaximized(await win.isMaximized());
    } catch {
      /* ignore */
    }
  }

  async function minimize() {
    try {
      await win.minimize();
    } catch {
      /* ignore */
    }
  }

  async function closeWindow() {
    try {
      await win.close();
    } catch {
      /* ignore */
    }
  }

  return (
    <header className="titlebar" onMouseDown={onMouseDown} onDoubleClick={onDoubleClick}>
      <button type="button" className="titlebar-home" title="回到主页面" onClick={onHome}>
        <span className="logo sm">Fs</span>
        <strong>FerraSSH</strong>
      </button>
      <nav className="titlebar-actions">
        <button type="button" className={settingsActive ? "on" : ""} disabled={!settingsEnabled} onClick={onSettings}>
          设置
        </button>
        <button type="button" className={aiActive ? "on" : ""} disabled={!settingsEnabled} onClick={onAi}>
          AI
        </button>
        <button type="button" className={aboutActive ? "on" : ""} onClick={onAbout}>
          关于
        </button>
        <button type="button" onClick={onUpdate}>
          更新
        </button>
      </nav>
      <div className="titlebar-flex" />
      <div
        className="window-controls"
        onMouseDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
      >
        <button type="button" className="win-btn" title="最小化" aria-label="最小化" onClick={() => void minimize()}>
          <svg viewBox="0 0 10 10" aria-hidden>
            <path d="M1 5h8" />
          </svg>
        </button>
        <button
          type="button"
          className="win-btn"
          title={maximized ? "还原" : "最大化"}
          aria-label={maximized ? "还原" : "最大化"}
          onClick={() => void toggleMaximize()}
        >
          {maximized ? (
            <svg viewBox="0 0 10 10" aria-hidden>
              <path d="M2 3.5h5.5v5.5H2z" />
              <path d="M3.5 3.5V2h5.5v5.5H7.5" />
            </svg>
          ) : (
            <svg viewBox="0 0 10 10" aria-hidden>
              <path d="M1.5 1.5h7v7h-7z" />
            </svg>
          )}
        </button>
        <button type="button" className="win-btn close" title="关闭" aria-label="关闭" onClick={() => void closeWindow()}>
          <svg viewBox="0 0 10 10" aria-hidden>
            <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
          </svg>
        </button>
      </div>
    </header>
  );
}
