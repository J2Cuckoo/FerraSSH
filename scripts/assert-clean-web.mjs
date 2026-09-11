import { existsSync, readdirSync, statSync } from "node:fs";
import { extname, join } from "node:path";

const forbiddenExt = new Set([".db", ".sqlite", ".sqlite3", ".vault", ".pdb", ".sql"]);
const forbiddenName = /ferrassh\.(db|vault)|localstorage|appdata/i;

function walk(dir, acc = []) {
  if (!existsSync(dir)) return acc;
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, acc);
    else acc.push(p);
  }
  return acc;
}

const root = join(import.meta.dirname, "..", "web-dist");
const files = walk(root);
const bad = files.filter((p) => {
  const ext = extname(p).toLowerCase();
  return forbiddenExt.has(ext) || forbiddenName.test(p);
});
if (bad.length) {
  console.error("前端产物含有本地数据，拒绝打包：");
  for (const p of bad) console.error(" ", p);
  process.exit(1);
}
