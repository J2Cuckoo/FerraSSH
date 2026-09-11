import type { RemoteEntry } from "./types";

export type ConflictKind = "overwrite" | "overwrite_all" | "skip" | "skip_all" | "cancel";

type Props = {
  entry: RemoteEntry;
  dest: string;
  remaining: number;
  onPick: (kind: ConflictKind) => void;
};

export default function ConflictDialog({ entry, dest, remaining, onPick }: Props) {
  return (
    <div className="modal danger-modal" onClick={(e) => e.stopPropagation()}>
      <div className="card wide" onClick={(e) => e.stopPropagation()}>
        <h2>远端已存在</h2>
        <p className="hint">
          {entry.is_dir ? "文件夹" : "文件"} <b>{entry.name}</b> 已在服务器存在：
        </p>
        <p className="fp">{dest}</p>
        {remaining > 1 && <p className="muted">还剩 {remaining - 1} 项待确认。</p>}
        <div className="row card-actions wrap">
          <button type="button" className="primary" onClick={() => onPick("overwrite")}>
            覆盖
          </button>
          <button type="button" onClick={() => onPick("overwrite_all")}>
            全部覆盖
          </button>
          <button type="button" onClick={() => onPick("skip")}>
            跳过
          </button>
          <button type="button" onClick={() => onPick("skip_all")}>
            全部跳过
          </button>
          <button type="button" onClick={() => onPick("cancel")}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
