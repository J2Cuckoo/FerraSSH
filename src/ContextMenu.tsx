import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { clampMenu } from "./menuPos";

type Props = {
  x: number;
  y: number;
  onClose?: () => void;
  children: ReactNode;
  className?: string;
};

export default function ContextMenu({ x, y, children, className }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(() => ({ left: x, top: y }));

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setPos(clampMenu(x, y, el.offsetWidth, el.offsetHeight));
  }, [x, y, children]);

  return (
    <div
      ref={ref}
      className={className ? `ctx ${className}` : "ctx"}
      style={{ left: pos.left, top: pos.top }}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {children}
    </div>
  );
}
