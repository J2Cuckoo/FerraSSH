/** Default terminal foreground (NamedColor::Foreground). */
export const DEFAULT_TERM_FG = 0xd6deeb;
export const DEFAULT_TERM_FG_HEX = "#d6deeb";

/** Eye-care replacements for default white only — not ANSI / truecolor. */
export const EYE_CARE_FGS = [
  { id: "default", name: "默认", color: "#d6deeb" },
  { id: "warm", name: "暖白", color: "#e6d5b8" },
  { id: "paper", name: "米黄", color: "#dcc48a" },
  { id: "amber", name: "琥珀", color: "#c9ae62" },
  { id: "leaf", name: "叶绿", color: "#b4c882" },
] as const;

export type EyeCareId = (typeof EYE_CARE_FGS)[number]["id"];

export function resolveEyeCareFg(hex?: string | null): number {
  const n = (hex || "").trim().toLowerCase();
  const found = EYE_CARE_FGS.find((c) => c.color === n);
  return parseInt((found?.color || DEFAULT_TERM_FG_HEX).slice(1), 16);
}

export function isDefaultTermFg(rgb: number): boolean {
  return !rgb || rgb === DEFAULT_TERM_FG;
}
