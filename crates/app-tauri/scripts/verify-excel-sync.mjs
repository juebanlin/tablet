// Excel 编辑会话（导出 → 监控 mtime → 自动回写）自验。
// 不真正拉起 Excel：用 open=false 建会话，然后直接 touch 文件模拟 Excel 保存。
import fs from "node:fs";

const PORT = process.env.CDP_PORT || 9222;

const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("没有找到页面 target");

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});

let seq = 0;
const pending = new Map();
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
};

function send(method, params = {}) {
  return new Promise((resolve) => {
    const id = ++seq;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evalJs(expression) {
  const msg = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (msg.result?.exceptionDetails) {
    throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
  }
  return msg.result?.result?.value;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function until(expr, label, timeout = 10000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await evalJs(expr)) return true;
    await sleep(150);
  }
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

await evalJs(`(async () => {
  __tt.selectTree("table", "demo", "hero", "HeroBase");
  await new Promise(r => setTimeout(r, 900));
  return true;
})()`);
const file = await evalJs(`window.__TAURI__.core.invoke("excel_begin_edit", { project: "demo", group: "hero", open: false })`);
check("导出并建立会话", fs.existsSync(file), file);

const editing = await evalJs(`(async () => {
  const s = await window.__TAURI__.core.invoke('snapshot');
  return s.status.excel_editing;
})()`);
check("会话状态", editing.includes("hero"), editing);

// 模拟 Excel 保存：把 mtime 往后拨
const t = new Date(Date.now() + 5000);
fs.utimesSync(file, t, t);

const changed = await evalJs(`window.__TAURI__.core.invoke('excel_poll')`);
check("轮询发现改动", changed.some((i) => i.group === "hero"), changed);

const snap = await evalJs(`window.__TAURI__.core.invoke('excel_sync_group', { project: 'demo', group: 'hero' })`);
check("同步导入", !!snap.grid, !!snap.grid);

const ended = await evalJs(`(async () => {
  const s = await window.__TAURI__.core.invoke('excel_end_edit', { project: 'demo', group: 'hero' });
  return s.status.excel_editing;
})()`);
check("结束会话", ended.length === 0, ended);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
