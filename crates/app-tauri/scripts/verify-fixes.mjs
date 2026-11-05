// 拖动选区 / 引用选择器 / 拼音搜索 / 公式栏自验。
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
  console.log("DIAG", await evalJs(`document.body.innerText.slice(0, 500)`));
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

async function drag(fromSel, toSel) {
  const r = await evalJs(`(() => {
    const a = document.querySelector(${JSON.stringify(fromSel)});
    const b = document.querySelector(${JSON.stringify(toSel)});
    if (!a || !b) return null;
    const ra = a.getBoundingClientRect(), rb = b.getBoundingClientRect();
    return {
      x1: ra.left + ra.width / 2, y1: ra.top + ra.height / 2,
      x2: rb.left + rb.width / 2, y2: rb.top + rb.height / 2
    };
  })()`);
  if (!r) throw new Error("drag target missing");
  await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", buttons: 1, clickCount: 1, x: r.x1, y: r.y1 });
  for (let i = 1; i <= 8; i++) {
    await send("Input.dispatchMouseEvent", {
      type: "mouseMoved", button: "left", buttons: 1,
      x: r.x1 + ((r.x2 - r.x1) * i) / 8,
      y: r.y1 + ((r.y2 - r.y1) * i) / 8,
    });
  }
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", buttons: 0, clickCount: 1, x: r.x2, y: r.y2 });
}

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 选中 Building
await evalJs(`(async () => {
  __tt.selectTree('table', 'demo', 'building', 'Building');
  await new Promise(r => setTimeout(r, 1200));
  return true;
})()`);

// 1. 拖动选区 B1 -> D4，且不产生原生文本选择
await drag(`td[data-cell="0:1"]`, `td[data-cell="3:3"]`);
await sleep(200);
const dragInfo = await evalJs(`({
  info: __tt.grid.selectionInfo(),
  native: window.getSelection().toString(),
  cells: document.querySelectorAll('td.cell-selected, td.in-range').length
})`);
check(
  "拖动选区",
  dragInfo.info.startsWith("B1:D4") && dragInfo.native === "" && dragInfo.cells === 12,
  dragInfo,
);

// 2. 引用选择器：双击 kind 单元格
await evalJs(`(() => {
  const td = document.querySelector('td[data-cell="0:2"]');
  td.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  return true;
})()`);
await until(`!!document.querySelector('.ref-popup')`, "引用选择器");
const ref = await evalJs(`({
  options: [...document.querySelectorAll('.ref-option')].map(o => o.innerText),
  hasSearch: !!document.querySelector('.ref-popup input')
})`);
check(
  "引用选择器",
  ref.hasSearch && ref.options.length === 3 && ref.options.some((o) => o.includes("MILITARY")),
  ref,
);
await evalJs(`document.querySelector('.ref-option[data-id="2"]').click()`);
await evalJs(`document.querySelector('[data-ref-ok]').click()`);
await evalJs(`__tt.runOp('snapshot', {})`);
const refSaved = await evalJs(`__tt.state.snapshot.grid.rows[0][2]`);
check("引用选择器提交", refSaved === "2", refSaved);

// 3. 拼音搜索：新建「英雄组」，搜 yxz
await evalJs(`__tt.runOp('new_group', { project: 'demo', name: '英雄组', xlsx: false })`);
await evalJs(`(() => {
  const s = document.getElementById('tree-search');
  s.value = 'yxz';
  s.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
await sleep(300);
const pinyin = await evalJs(`document.getElementById('tree-list').innerText.includes('英雄组')`);
check("拼音首字母搜索", pinyin);

// 4. 公式栏：textarea + 展开态
const formula = await evalJs(`(() => {
  __tt.grid.selectCell(0, 1);
  const el = document.getElementById('formula-input');
  return { tag: el.tagName, textarea: el.tagName === 'TEXTAREA' };
})()`);
check("公式栏 textarea", formula.textarea, formula);

// 5. 列表引用多选：HeroBase.items（List<@Item>）
await evalJs(`(async () => {
  __tt.selectTree("table", "demo", "hero", "HeroBase");
  await new Promise(r => setTimeout(r, 900));
  return true;
})()`);
await evalJs(`(() => {
  const td = document.querySelector('td[data-cell="0:11"]');
  td.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  return true;
})()`);
await until(`!!document.querySelector('.ref-popup [data-ref-ok]')`, "列表引用选择器");
const multi = await evalJs(`document.querySelectorAll('.ref-popup .ref-option').length`);
check("列表引用多选", multi > 0, { options: multi });
await evalJs(`(() => {
  const opt = document.querySelector('.ref-option[data-id="2003"]');
  if (opt) opt.click();
  document.querySelector('[data-ref-ok]').click();
  return true;
})()`);
await evalJs(`__tt.runOp('snapshot', {})`);
const multiValue = await evalJs(`__tt.state.snapshot.grid.rows[0][11]`);
check("列表引用提交", multiValue === "2001;2002;2003", multiValue);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
