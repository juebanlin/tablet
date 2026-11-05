// 项目设置（分隔符 / 导出）自验（CDP 驱动真实页面）。
// 前提：带 --remote-debugging-port=9222 启动 tablet-tauri。
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
    settingsOpen: !document.getElementById('project-settings').hidden,
    error: document.getElementById('ps-error').textContent,
    seps: document.querySelectorAll('#ps-seps .sep-row').length,
    status: __tt.state.snapshot.status,
    logs: __tt.state.snapshot.logs.slice(0, 6).map(l => l.level + ' ' + l.message)
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

// 打开项目设置
await evalJs(`(() => {
  const el = document.querySelector('.tree-item[data-key^="p:"]') || document.querySelectorAll('.tree-item')[0];
  const r = el.getBoundingClientRect();
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
})()`);
await sleep(200);
await evalJs(`[...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes('项目设置')).click()`);
await until(`!document.getElementById('project-settings').hidden`, "项目设置打开");

// 分隔符 tab：应有 25 项
await evalJs(`document.querySelector('[data-pstab=sep]').click()`);
const seps = await evalJs(`({
  rows: document.querySelectorAll('#ps-seps .sep-row').length,
  def: document.querySelector('#ps-seps input[data-sep-key="List"]').placeholder
})`);
check("分隔符 25 项", seps.rows === 25 && seps.def === ";", seps);

// 改一个不影响现有数据的项：Set → |
await evalJs(`(() => {
  const input = document.querySelector('#ps-seps input[data-sep-key="List_Tuple4.tuple"]');
  input.value = '|';
  return true;
})()`);

// 导出 tab
await evalJs(`document.querySelector('[data-pstab=export]').click()`);
await evalJs(`(() => {
  const set = (id, v) => { const el = document.getElementById(id); el.value = v; el.dispatchEvent(new Event('change', { bubbles: true })); };
  set('ps-enc', 'gbk');
  set('ps-eol', 'crlf');
  document.getElementById('ps-server-data').value = 'qa/server/data';
  document.getElementById('ps-java-pkg').value = 'qa.pkg';
  document.getElementById('ps-java-lombok').checked = false;
  return true;
})()`);

// 校验 tab：警告级别改成「忽略」
await evalJs(`(() => {
  document.querySelector('[data-pstab=validate]').click();
  const el = document.getElementById('ps-val-warning');
  el.value = 'ignore';
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);

await evalJs(`document.getElementById('ps-ok').click()`);
await until(`document.getElementById('project-settings').hidden`, "保存项目设置");
await sleep(300);

// 重新打开验证回填
await evalJs(`(() => {
  const el = document.querySelectorAll('.tree-item')[0];
  const r = el.getBoundingClientRect();
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
})()`);
await sleep(200);
await evalJs(`[...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes('项目设置')).click()`);
await until(`!document.getElementById('project-settings').hidden`, "项目设置重新打开");
const persisted = await evalJs(`({
  set: document.querySelector('#ps-seps input[data-sep-key="List_Tuple4.tuple"]').value,
  enc: document.getElementById('ps-enc').value,
  eol: document.getElementById('ps-eol').value,
  data: document.getElementById('ps-server-data').value,
  javaPkg: document.getElementById('ps-java-pkg').value,
  lombok: document.getElementById('ps-java-lombok').checked,
  valWarning: document.getElementById('ps-val-warning').value
})`);
check(
  "设置回填",
  persisted.set === "|" &&
    persisted.enc === "gbk" &&
    persisted.eol === "crlf" &&
    persisted.data === "qa/server/data" &&
    persisted.javaPkg === "qa.pkg" &&
    persisted.lombok === false &&
    persisted.valWarning === "ignore",
  persisted,
);
const initialRevert = await evalJs(`({
  revs: document.querySelectorAll('.ps-rev').length,
  hasAll: !!document.getElementById('ps-revert-all')
})`);
check("字段还原按钮", initialRevert.revs > 0 && initialRevert.hasAll, initialRevert);
await evalJs(`(() => {
  const el = document.getElementById('ps-name');
  el.value = '临时名称';
  el.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
const dirtyField = await evalJs(`({
  mod: document.getElementById('ps-name').classList.contains('ps-mod'),
  all: document.getElementById('ps-revert-all').disabled
})`);
check("修改标记与全部还原可用", dirtyField.mod && !dirtyField.all, dirtyField);
await evalJs(`document.querySelector('[data-ps-rev="name"]').click()`);
await sleep(150);
const reverted = await evalJs(`({
  value: document.getElementById('ps-name').value,
  mod: document.getElementById('ps-name').classList.contains('ps-mod')
})`);
check("单字段还原", reverted.value !== "临时名称" && !reverted.mod, reverted);
await evalJs(`document.getElementById('ps-ok').click()`);

// 保存工作区，落盘
await evalJs(`__tt.runOp('save_workspace', {})`);
await until(`__tt.state.snapshot.status.dirty === false`, "保存工作区");
check("保存工作区", true);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
