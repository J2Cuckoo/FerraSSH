/** Clamp a context menu so it stays fully visible in the viewport. */
export function clampMenu(x: number, y: number, w: number, h: number, pad = 8): { left: number; top: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left = x;
  let top = y;
  if (left + w > vw - pad) left = Math.max(pad, vw - w - pad);
  if (top + h > vh - pad) top = Math.max(pad, vh - h - pad);
  if (left < pad) left = pad;
  if (top < pad) top = pad;
  return { left, top };
}
