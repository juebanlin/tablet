// 新建项目来源切换 / 多列横向滚动 / 拖选样式自验（CDP 驱动真实页面）。
// 需要全新工作区（脚本会插入列、创建项目）：WORKDIR=<工作区绝对路径>
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

// 1. 新建项目：切换来源要清空上一来源的预览 / 身份 / 预设勾选。
await evalJs(`document.getElementById('btn-new-project').click()`);
await until(`!document.getElementById('create-project').hidden`, "新建项目对话框");
await evalJs(`document.querySelector('[data-cptab=file]').click()`);
await evalJs(`(() => {
  document.getElementById('cp-file').value = 'x.tblschema';
  document.getElementById('cp-id').value = 'foo';
  document.getElementById('cp-name').value = 'Foo';
  document.getElementById('cp-version').value = '1.2.3';
  document.getElementById('cp-preset').checked = true;
  return true;
})()`);
await evalJs(`document.querySelector('[data-cptab=template]').click()`);
const cleared = await evalJs(`({
  id: document.getElementById('cp-id').value,
  name: document.getElementById('cp-name').value,
  version: document.getElementById('cp-version').value,
  file: document.getElementById('cp-file').value,
  preset: document.getElementById('cp-preset').checked
})`);
check(
  "切换来源清空旧数据",
  !cleared.id && !cleared.name && !cleared.version && !cleared.file && !cleared.preset,
  cleared,
);
// 模板来源选一个模板会自动填身份；再切回空项目应再次清空。
await until(`document.querySelectorAll('#cp-tpl-list [data-tpl]').length > 0`, "模板列表");
await evalJs(`document.querySelector('#cp-tpl-list [data-tpl]').click()`);
await until(`document.getElementById('cp-id').value.length > 0`, "模板预览");
await evalJs(`document.querySelector('[data-cptab=empty]').click()`);
const afterEmpty = await evalJs(`({
  id: document.getElementById('cp-id').value,
  name: document.getElementById('cp-name').value
})`);
check("模板 -> 空项目清空身份", !afterEmpty.id && !afterEmpty.name, afterEmpty);
await evalJs(`document.getElementById('cp-cancel').click()`);
await sleep(150);

// 2. 插入多列：列字母 Excel 式、可横向滚动、插入后新列自动可见。
await evalJs(`(async () => {
  __tt.selectTree('table', 'demo', 'hero', 'Hero');
  await new Promise(r => setTimeout(r, 900));
  return true;
})()`);
for (let i = 0; i < 20; i++) {
  await evalJs(`(async () => { await __tt.runOp('insert_column', { at: __tt.grid.cols }); return true })()`);
}
const wide = await evalJs(`(() => {
  const s = document.getElementById('grid-scroll');
  const ths = [...document.querySelectorAll('th[data-col]')];
  return { cols: __tt.grid.cols, sw: s.scrollWidth, cw: s.clientWidth,
    letter: ths[26]?.textContent, last: ths[ths.length - 1]?.textContent };
})()`);
check("超过 Z 的列字母", wide.cols > 26 && wide.letter === "AA", wide);
const scrolled = await evalJs(`(() => {
  const s = document.getElementById('grid-scroll');
  s.scrollLeft = s.scrollWidth;
  return s.scrollLeft;
})()`);
check("横向可滚到最右", scrolled > 0, scrolled);

// 右边界右键插入列：新列应在可视区内。
await evalJs(`(() => {
  const s = document.getElementById('grid-scroll');
  s.scrollLeft = s.scrollWidth;
  const th = [...document.querySelectorAll('th[data-col]')].pop();
  const r = th.getBoundingClientRect();
  th.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 5, clientY: r.top + 5 }));
  return true;
})()`);
await until(`!!document.querySelector('.ctx-item')`, "列右键菜单");
await evalJs(`(() => {
  [...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes('在右侧插入列')).click();
  return true;
})()`);
await sleep(600);
const revealed = await evalJs(`(() => {
  const s = document.getElementById('grid-scroll');
  const th = [...document.querySelectorAll('th[data-col]')].pop();
  const r = th.getBoundingClientRect();
  const b = s.getBoundingClientRect();
  return { visible: r.right <= b.right + 1 && r.left >= b.left, left: s.scrollLeft, max: s.scrollWidth - s.clientWidth };
})()`);
check("插入后新列自动可见", revealed.visible && revealed.left >= revealed.max - 40, revealed);

// 3. 拖选：选区半透明（网格线还在）、当前格不再画描边。
await evalJs(`(() => {
  const s = document.getElementById('grid-scroll');
  s.scrollLeft = 0;
  s.scrollTop = 0;
  __tt.grid.kind = 'none';
  __tt.grid.applySelection();
  return true;
})()`);
await sleep(200);
const box = await evalJs(`(() => {
  const a = document.querySelector('td[data-cell="0:1"]').getBoundingClientRect();
  const b = document.querySelector('td[data-cell="3:3"]').getBoundingClientRect();
  return { x1: a.left + a.width / 2, y1: a.top + a.height / 2, x2: b.left + b.width / 2, y2: b.top + b.height / 2 };
})()`);
await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", buttons: 1, clickCount: 1, x: box.x1, y: box.y1 });
await sleep(80);
for (let i = 1; i <= 8; i++) {
  await send("Input.dispatchMouseEvent", {
    type: "mouseMoved", button: "left", buttons: 1,
    x: box.x1 + ((box.x2 - box.x1) * i) / 8,
    y: box.y1 + ((box.y2 - box.y1) * i) / 8,
  });
  await sleep(20);
}
await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", buttons: 0, clickCount: 1, x: box.x2, y: box.y2 });
const sel = await evalJs(`(() => {
  const ir = document.querySelector('td[data-cell="1:2"]');
  const foc = document.querySelector('td[data-cell="3:3"]');
  const a = getComputedStyle(ir);
  const b = getComputedStyle(foc);
  return { info: __tt.grid.selectionInfo(), rangeBg: a.backgroundColor, border: a.borderRightColor,
    focusBg: b.backgroundColor, focusShadow: b.boxShadow };
})()`);
check("选区半透明、网格线仍在", sel.info.startsWith("B1:D4") && sel.rangeBg.startsWith("rgba(") && sel.border === "rgb(229, 231, 235)", sel);
check("当前格无描边", sel.focusShadow === "none", sel);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
