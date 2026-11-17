// 导出对话框格式勾选持久化自验：改选 → 关闭 → 重开沿用；预设也落盘。
import fs from "node:fs";

const PORT = process.env.CDP_PORT || 9222;
const DATA = process.env.USER_DATA || "";
const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("没有找到页面 target");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let seq = 0; const pending = new Map();
ws.onmessage = (ev) => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const send = (method, params = {}) => new Promise((res) => { const id = ++seq; pending.set(id, res); ws.send(JSON.stringify({ id, method, params })); });
async function evalJs(expression) {
  const msg = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (msg.result?.exceptionDetails) throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
  return msg.result?.result?.value;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(expr, label, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) { if (await evalJs(expr)) return; await sleep(120); }
  throw new Error(`超时: ${label}`);
}
const results = [];
const check = (name, cond, detail) => { results.push({ name, ok: !!cond }); if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail)); };

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 1. 清空后只勾 json + lua，关闭再打开应沿用。
await evalJs(`document.getElementById('btn-export').click()`);
await until(`!document.getElementById('export-dialog').hidden`, "导出对话框打开");
await evalJs(`document.getElementById('ex-none').click()`);
await evalJs(`(() => { for (const f of ['json','lua']) { const i=document.querySelector('#ex-formats input[data-fmt="'+f+'"]'); i.checked=true; i.dispatchEvent(new Event('change',{bubbles:true})); } })()`);
await sleep(400);
await evalJs(`document.getElementById('ex-cancel').click()`);
await evalJs(`document.getElementById('btn-export').click()`);
await until(`!document.getElementById('export-dialog').hidden`, "再次打开");
await sleep(300);
const first = await evalJs(`[...document.querySelectorAll('#ex-formats input[data-fmt]')].filter(i=>i.checked).map(i=>i.dataset.fmt)`);
check("重开沿用上次勾选", JSON.stringify(first) === JSON.stringify(["json","lua"]), first);

// 2. 预设「数据」也应立即落盘。
await evalJs(`document.querySelector('#export-dialog [data-expre="data"]').click()`);
await sleep(400);
await evalJs(`document.getElementById('ex-cancel').click()`);
await evalJs(`document.getElementById('btn-export').click()`);
await until(`!document.getElementById('export-dialog').hidden`, "第三次打开");
await sleep(300);
const preset = await evalJs(`[...document.querySelectorAll('#ex-formats input[data-fmt]')].filter(i=>i.checked).map(i=>i.dataset.fmt)`);
check("预设批量勾选落盘", JSON.stringify(preset) === JSON.stringify(["json","xml","bin"]), preset);
await evalJs(`document.getElementById('ex-cancel').click()`);

// 3. 文件内容带 ui.export.formats。
if (DATA) {
  const text = fs.readFileSync(DATA, "utf8");
  const data = JSON.parse(text);
  const formats = data?.ui?.export?.formats || {};
  check("user-data.json 记录格式", formats.json === true && formats.xml === true && formats.bin === true && formats.lua === false, formats);
}

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
