import { deflateSync } from "node:zlib";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
const script = join(dirname(fileURLToPath(import.meta.url)), "render-fs-logo.ps1");
mkdirSync(root, { recursive: true });

const BG = [61, 205, 195, 255];
const FG = [6, 32, 30, 255];
const CLEAR = [0, 0, 0, 0];

function crc32(buf) {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

function png(size, paint) {
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[(size * 4 + 1) * y] = 0;
    for (let x = 0; x < size; x++) {
      const [r, g, b, a] = paint(x, y, size);
      const o = (size * 4 + 1) * y + 1 + x * 4;
      raw[o] = r;
      raw[o + 1] = g;
      raw[o + 2] = b;
      raw[o + 3] = a;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function inRoundRect(x, y, s, r) {
  const xi = Math.min(x, s - 1 - x);
  const yi = Math.min(y, s - 1 - y);
  if (xi >= r || yi >= r) return true;
  const dx = r - 0.5 - xi;
  const dy = r - 0.5 - yi;
  return dx * dx + dy * dy <= r * r;
}

const F = [
  "11111",
  "10000",
  "11110",
  "10000",
  "10000",
];
const S = [
  "01110",
  "10000",
  "01110",
  "00001",
  "11110",
];

function stamp(grid, px, py, scale, x, y) {
  const gx = Math.floor((x - px) / scale);
  const gy = Math.floor((y - py) / scale);
  if (gy < 0 || gy >= grid.length || gx < 0 || gx >= grid[0].length) return false;
  return grid[gy][gx] === "1";
}

function paintLogo(x, y, s) {
  const r = Math.max(1, Math.round(s * 0.1875));
  if (!inRoundRect(x, y, s, r)) return CLEAR;
  const scale = Math.max(1, Math.round(s / 12));
  const letterW = 5 * scale;
  const letterH = 5 * scale;
  const gap = Math.max(1, Math.round(scale * 0.7));
  const total = letterW * 2 + gap;
  const px = Math.round((s - total) / 2);
  const py = Math.round((s - letterH) / 2);
  if (stamp(F, px, py, scale, x, y) || stamp(S, px + letterW + gap, py, scale, x, y)) return FG;
  return BG;
}

function writeIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + dir.length;
  entries.forEach((e, i) => {
    const o = i * 16;
    dir[o] = e.size >= 256 ? 0 : e.size;
    dir[o + 1] = e.size >= 256 ? 0 : e.size;
    dir.writeUInt16LE(0, o + 2);
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(e.buf.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += e.buf.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.buf)]);
}

function writeIcns(parts) {
  const inner = Buffer.concat(
    parts.map(({ type, buf }) => {
      const size = Buffer.alloc(4);
      size.writeUInt32BE(8 + buf.length);
      return Buffer.concat([Buffer.from(type), size, buf]);
    }),
  );
  const total = Buffer.alloc(4);
  total.writeUInt32BE(8 + inner.length);
  return Buffer.concat([Buffer.from("icns"), total, inner]);
}

function renderWithGdi() {
  if (process.platform !== "win32") return false;
  const r = spawnSync(
    "powershell",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, "-OutDir", root],
    { encoding: "utf8" },
  );
  if (r.status !== 0) {
    console.warn(r.stderr || r.stdout || "GDI icon render failed, using fallback");
    return false;
  }
  return existsSync(join(root, "32x32.png")) && existsSync(join(root, "128x128.png"));
}

const gdiOk = renderWithGdi();
if (!gdiOk) {
  writeFileSync(join(root, "16x16.png"), png(16, paintLogo));
  writeFileSync(join(root, "32x32.png"), png(32, paintLogo));
  writeFileSync(join(root, "48x48.png"), png(48, paintLogo));
  writeFileSync(join(root, "64x64.png"), png(64, paintLogo));
  writeFileSync(join(root, "128x128.png"), png(128, paintLogo));
  writeFileSync(join(root, "256x256.png"), png(256, paintLogo));
}

const p32 = readFileSync(join(root, "32x32.png"));
const p128 = readFileSync(join(root, "128x128.png"));
const p256 = existsSync(join(root, "256x256.png")) ? readFileSync(join(root, "256x256.png")) : png(256, paintLogo);
writeFileSync(join(root, "128x128@2x.png"), p256);
if (!gdiOk || !existsSync(join(root, "icon.ico"))) {
  const p16 = existsSync(join(root, "16x16.png")) ? readFileSync(join(root, "16x16.png")) : p32;
  const p48 = existsSync(join(root, "48x48.png")) ? readFileSync(join(root, "48x48.png")) : p32;
  writeFileSync(
    join(root, "icon.ico"),
    writeIco([
      { size: 16, buf: p16 },
      { size: 32, buf: p32 },
      { size: 48, buf: p48 },
      { size: 256, buf: p256 },
    ]),
  );
}
writeFileSync(join(root, "icon.icns"), writeIcns([
  { type: "ic11", buf: p32 },
  { type: "ic07", buf: p128 },
  { type: "ic08", buf: p256 },
]));
console.log("icons written to", root);
