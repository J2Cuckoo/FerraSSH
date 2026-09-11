import { exit } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";

const HOMEPAGE = "https://hyrubik.com/#/products/ferrassh";

async function openOfficial() {
  try {
    await openUrl(HOMEPAGE);
  } catch {
    window.open(HOMEPAGE, "_blank", "noopener,noreferrer");
  }
}

async function closeAndQuit() {
  try {
    await exit(0);
  } catch {
    window.close();
  }
}

export default function SealFail() {
  return (
    <div className="seal-fail" data-tauri-drag-region>
      <div className="seal-fail-card" onMouseDown={(e) => e.stopPropagation()}>
        <p>
          为保证您和他人的数据安全，请不要直接使用非官方安装包，请前往
          <button type="button" className="link" onClick={() => void openOfficial()}>
            【官方网站】
          </button>
          下载安装包。
        </p>
        <button type="button" className="primary" onClick={() => void closeAndQuit()}>
          关闭
        </button>
      </div>
    </div>
  );
}
