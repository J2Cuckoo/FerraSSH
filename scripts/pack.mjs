import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const VERSION = pkg.version;
const UPDATE_BASE = "https://files.hyrubik.com/updates/ferrassh";

const PLATFORMS = {
  windows: {
    host: "win32",
    bundles: "nsis",
    outputs: "与开发测试相同的 Tauri 界面完整安装包（NSIS + WebView2）",
  },
  macos: {
    host: "darwin",
    bundles: "app,dmg",
    outputs: "与 Windows 相同的 Tauri 界面（.app + .dmg）",
  },
  linux: {
    host: "linux",
    bundles: "deb,appimage",
    outputs: "与 Windows 相同的 Tauri 界面（.deb + AppImage）",
  },
};

const aliases = { win: "windows", win32: "windows", mac: "macos", darwin: "macos", osx: "macos", gnu: "linux" };

function usage(code = 0) {
  console.log(`FerraSSH 打包（三平台同一套 Tauri + React 界面，不含本机测试数据）

  npm run pack:windows      NSIS 完整安装包 → dist/（与 latest.json 同级）
  npm run pack:macos        .app + .dmg     → dist/
  npm run pack:linux        .deb + AppImage → dist/
`);
  process.exit(code);
}

function detect() {
  if (process.platform === "win32") return "windows";
  if (process.platform === "darwin") return "macos";
  if (process.platform === "linux") return "linux";
  throw new Error(process.platform);
}

function run(cmd, args, extraEnv = {}) {
  const r = spawnSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32",
    env: {
      ...process.env,
      CARGO_PROFILE_RELEASE_STRIP: "true",
      CARGO_PROFILE_RELEASE_DEBUG: "0",
      ...extraEnv,
    },
  });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}

function parse(argv) {
  let name = "";
  for (const a of argv) {
    if (a === "-h" || a === "--help") usage(0);
    if (!a.startsWith("-") && !name) name = a;
  }
  return name;
}

function collectFiles(dir, acc = []) {
  if (!existsSync(dir)) return acc;
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) collectFiles(p, acc);
    else acc.push(p);
  }
  return acc;
}

function firstFile(dir, re) {
  const files = collectFiles(dir).filter((p) => re.test(p));
  if (!files.length) return "";
  const ver = VERSION.replace(/\./g, "[._]");
  const matched = files.find((p) => new RegExp(ver, "i").test(basename(p)));
  if (matched) return matched;
  return files.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}

function copyNamed(src, destDir, destName) {
  mkdirSync(destDir, { recursive: true });
  const dest = join(destDir, destName);
  cpSync(src, dest, { recursive: true });
  return dest;
}

function sha256File(p) {
  return createHash("sha256").update(readFileSync(p)).digest("hex");
}

function readUpdateNotes() {
  const p = join(root, "scripts", "update-notes.txt");
  if (!existsSync(p)) return `FerraSSH ${VERSION}`;
  return readFileSync(p, "utf8").replace(/\r\n/g, "\n").trim() || `FerraSSH ${VERSION}`;
}

function writeLatestJson(copied) {
  const setup = copied.find((p) => /setup\.exe$/i.test(p));
  const dmg = copied.find((p) => /\.dmg$/i.test(p));
  const appimage = copied.find((p) => /\.AppImage$/i.test(p));
  const deb = copied.find((p) => /\.deb$/i.test(p));
  const asset = (p) => {
    const name = basename(p);
    return { url: `${UPDATE_BASE}/${name}`, sha256: sha256File(p) };
  };
  const manifest = {
    version: VERSION,
    notes: readUpdateNotes(),
  };
  if (setup) {
    const win = asset(setup);
    manifest.url = win.url;
    manifest.windows = win;
  }
  if (dmg) manifest.macos = asset(dmg);
  if (appimage || deb) manifest.linux = asset(appimage || deb);
  const dest = join(root, "dist", "latest.json");
  mkdirSync(join(root, "dist"), { recursive: true });
  writeFileSync(dest, `${JSON.stringify(manifest, null, 2)}\n`);
  return dest;
}

function assertNoUserData(dir) {
  const bad = collectFiles(dir).filter((p) => /\.(db|sqlite|sqlite3|vault)$/i.test(p) || /ferrassh\.(db|vault)/i.test(p));
  if (bad.length) {
    console.error("安装包目录含有本地数据，已中止：");
    for (const p of bad) console.error(" ", p);
    process.exit(1);
  }
}

const raw = (parse(process.argv.slice(2)) || detect()).toLowerCase();
const key = aliases[raw] || raw;
const spec = PLATFORMS[key];
if (!spec) {
  console.error(`未知平台 ${raw}`);
  usage(1);
}
if (process.platform !== spec.host) {
  console.error(`请在 ${key} 本机执行打包（与 Windows 相同：用该系统的 npm run pack:${key}）。`);
  process.exit(1);
}

const outDir = join(root, "dist");
const nested = join(outDir, key);
const webDist = join(root, "web-dist");
mkdirSync(outDir, { recursive: true });
if (existsSync(nested)) rmSync(nested, { recursive: true, force: true });
if (existsSync(webDist)) {
  for (const name of readdirSync(webDist)) {
    if (name === ".gitkeep") continue;
    rmSync(join(webDist, name), { recursive: true, force: true });
  }
}

console.log(`打包 ${key}：${spec.outputs}\n`);
const sealName = randomBytes(16).toString("hex");
const sealSalt = randomBytes(32).toString("hex");
run("npx", ["tauri", "build", "--bundles", spec.bundles], {
  FERRA_SEAL_NAME: sealName,
  FERRA_SEAL_SALT: sealSalt,
});

const bundleRoot = join(root, "target", "release", "bundle");
const copied = [];

if (key === "windows") {
  const setup = firstFile(join(bundleRoot, "nsis"), /setup\.exe$/i);
  if (!setup) {
    console.error(`未找到 NSIS 安装包：${join(bundleRoot, "nsis")}`);
    process.exit(1);
  }
  copied.push(copyNamed(setup, outDir, `FerraSSH-${VERSION}-x64-Setup.exe`));
} else if (key === "macos") {
  const dmg = firstFile(join(bundleRoot, "dmg"), /\.dmg$/i);
  const app = firstFile(join(bundleRoot, "macos"), /\.app$/i) || collectFiles(join(bundleRoot, "macos")).find((p) => p.endsWith(".app"));
  const appDir = existsSync(join(bundleRoot, "macos", "FerraSSH.app")) ? join(bundleRoot, "macos", "FerraSSH.app") : "";
  if (dmg) copied.push(copyNamed(dmg, outDir, `FerraSSH-${VERSION}.dmg`));
  const appSrc = appDir || app;
  if (appSrc && existsSync(appSrc)) copied.push(copyNamed(appSrc, outDir, "FerraSSH.app"));
  if (!copied.length) {
    console.error(`未找到 macOS 安装包：${join(bundleRoot, "dmg")} 或 macos`);
    process.exit(1);
  }
} else {
  const deb = firstFile(join(bundleRoot, "deb"), /\.deb$/i);
  const appimage = firstFile(join(bundleRoot, "appimage"), /\.AppImage$/i);
  if (deb) copied.push(copyNamed(deb, outDir, `FerraSSH-${VERSION}-amd64.deb`));
  if (appimage) copied.push(copyNamed(appimage, outDir, `FerraSSH-${VERSION}-x86_64.AppImage`));
  if (!copied.length) {
    console.error(`未找到 Linux 安装包：${bundleRoot}`);
    process.exit(1);
  }
}

assertNoUserData(outDir);
assertNoUserData(webDist);

const latest = writeLatestJson(copied);

console.log(`\n安装包：`);
for (const p of copied) console.log(`  ${p}`);
console.log(`  ${latest}`);
console.log("把 latest.json 与安装包上传到 https://files.hyrubik.com/updates/ferrassh/");
console.log("两者必须同级，不要带 windows/ macos/ linux/ 子目录。");
console.log("本机保险库（%APPDATA%/Ferra/FerraSSH 等）不会打进安装包。");
