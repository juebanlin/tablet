// 全局设置 / 导出执行自验（CDP 驱动真实页面）。
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

async function until(expr, label, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await evalJs(expr)) return true;
    await sleep(150);
  }
  const diag = await evalJs(`JSON.stringify({
    gs: !document.getElementById('global-settings').hidden,
    ex: !document.getElementById('export-dialog').hidden,
    error: document.getElementById('gs-error').textContent + '|' + document.getElementById('ex-error').textContent,
    result: document.getElementById('ex-result').textContent
  })`);
  console.log("DIAG", diag);
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 1. 全局设置默认值
await evalJs(`document.getElementById('btn-global-settings').click()`);
await until(`!document.getElementById('global-settings').hidden`, "全局设置打开");
const defaults = await evalJs(`({
  header: document.getElementById('gs-header-trigger').value,
  data: document.getElementById('gs-data-trigger').value,
  level: document.getElementById('gs-log-level').value,
  seps: document.querySelectorAll('#gs-seps .sep-row').length
})`);
check(
  "全局设置默认值",
  defaults.header === "single" && defaults.data === "double" && !!defaults.level && defaults.seps === 25,
  defaults,
);

// 1.1 全局导出模板页
await evalJs(`document.querySelector('[data-gstab=export]').click()`);
const exportTab = await evalJs(`({
  rows: document.querySelectorAll('#gs-export .form-row').length,
  enc: !!document.getElementById('gs-enc')
})`);
check("全局导出模板页", exportTab.rows > 20 && exportTab.enc, exportTab);
await evalJs(`document.querySelector('[data-gstab=ui]').click()`);

// 2. 修改并保存
await evalJs(`(() => {
  document.getElementById('gs-log-level').value = 'debug';
  document.getElementById('gs-data-trigger').value = 'single';
  document.getElementById('gs-realtime').checked = true;
  document.getElementById('gs-ok').click();
  return true;
})()`);
await until(`document.getElementById('global-settings').hidden`, "保存全局设置");

// 重新打开验证
await evalJs(`document.getElementById('btn-global-settings').click()`);
await until(`!document.getElementById('global-settings').hidden`, "重新打开全局设置");
const saved = await evalJs(`({
  level: document.getElementById('gs-log-level').value,
  data: document.getElementById('gs-data-trigger').value,
  realtime: document.getElementById('gs-realtime').checked
})`);
check("全局设置回填", saved.level === "debug" && saved.data === "single" && saved.realtime, saved);
await evalJs(`document.getElementById('gs-cancel').click()`);

// 3. 数据选择器改成单击后，Constant 的 type 列单击即出类型对话框
await evalJs(`(async () => {
  __tt.selectTree('constant', 'demo', 'global', 'GameConst');
  await new Promise(r => setTimeout(r, 800));
  const td = document.querySelector('td[data-cell="0:1"]');
  const r = td.getBoundingClientRect();
  td.dispatchEvent(new PointerEvent('pointerdown', {
    bubbles: true, cancelable: true, button: 0, pointerId: 1,
    clientX: r.left + 6, clientY: r.top + 6
  }));
  return true;
})()`);
await sleep(300);
const picker = await evalJs(`!document.getElementById('type-selector').hidden`);
check("数据选择器单击呼出类型对话框", picker);
await evalJs(`document.getElementById('ts-cancel').click()`);

// 4. 导出：只选 JSON
await evalJs(`document.getElementById('btn-export').click()`);
await until(`!document.getElementById('export-dialog').hidden`, "导出对话框");
await evalJs(`(() => {
  document.querySelectorAll('#ex-formats input[data-fmt]').forEach(i => { i.checked = false; });
  document.querySelector('#ex-formats input[data-fmt=json]').checked = true;
  document.getElementById('ex-run').click();
  return true;
})()`);
await until(
  `document.getElementById('ex-result').textContent.includes('导出完成') || document.getElementById('ex-result').textContent.includes('失败')`,
  "导出完成",
  20000,
);
const report = await evalJs(`document.getElementById('ex-result').textContent`);
check("JSON 导出成功", report.includes("导出完成"), report);

await evalJs(`document.getElementById('ex-cancel').click()`);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
