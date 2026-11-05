// 表头类型/导出下拉 + 引用显示自验（CDP 驱动真实页面）。
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
    await sleep(120);
  }
  const diag = await evalJs(`JSON.stringify({
    header: document.querySelector('th[data-header="2:1"]')?.innerHTML?.slice(0, 200),
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

// 选中 hero/HeroBase
await evalJs(`(async () => {
  __tt.selectTree('table', 'demo', 'hero', 'HeroBase');
  await new Promise(r => setTimeout(r, 900));
  return true;
})()`);

const openHeader = async (header) => {
  await evalJs(`(() => {
    const th = document.querySelector('th[data-header="${header}"]');
    const r = th.getBoundingClientRect();
    th.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, cancelable: true, button: 0, pointerId: 1,
      clientX: r.left + 8, clientY: r.top + 8
    }));
    return true;
  })()`);
  await sleep(120);
};

// 1. 导出下拉：单击表头列 B 的导出行
await openHeader("1:1");
const exportMenu = await evalJs(`([...document.querySelectorAll('.cell-menu .cell-menu-item')].map(e => e.textContent))`);
check(
  "导出下拉出现",
  exportMenu.length === 4 && exportMenu.includes("服务器"),
  exportMenu,
);
await evalJs(`(() => {
  const it = [...document.querySelectorAll('.cell-menu .cell-menu-item')].find(e => e.textContent === '服务器');
  it.click();
  return true;
})()`);
await evalJs(`__tt.runOp('snapshot', {})`);
const exportSaved = await evalJs(`__tt.state.snapshot.grid.headers[1][1]`);
check("导出下拉提交", exportSaved === "服务器", exportSaved);

// 2. 类型选择器对话框：范式 + 参数 + 预览
await openHeader("2:1");
await until(`!document.getElementById('type-selector').hidden`, "类型选择器");
const ts = await evalJs(`({
  tabs: [...document.querySelectorAll('[data-tstab]')].map(b => b.dataset.tstab),
  paradigms: document.querySelectorAll('.ts-paradigm').length,
  result: document.getElementById('ts-result').textContent
})`);
check("类型选择器出现", ts.tabs.length === 2 && ts.paradigms > 10 && ts.result === "str", ts);
await evalJs(`(() => {
  [...document.querySelectorAll('.ts-paradigm')].find(e => e.textContent === 'List<_>').click();
  return true;
})()`);
await sleep(250);
await evalJs(`(() => {
  const s = document.querySelector('[data-tsslot="0"]');
  s.value = 'int';
  s.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(300);
const listPreview = await evalJs(`({
  result: document.getElementById('ts-result').textContent,
  java: document.getElementById('ts-java').textContent,
  go: document.getElementById('ts-go').textContent
})`);
check("容器参数与预览", listPreview.result === "List<int>" && listPreview.java.includes("List"), listPreview);
await evalJs(`document.getElementById('ts-ok').click()`);
await sleep(300);
check("类型对话框提交", (await evalJs(`__tt.state.snapshot.grid.headers[2][1]`)) === "List<int>");

// 3. 引用类型 tab
await openHeader("2:1");
await until(`!document.getElementById('type-selector').hidden`, "再次打开类型选择器");
await evalJs(`document.querySelector('[data-tstab=ref]').click()`);
await sleep(200);
await evalJs(`(() => {
  const it = [...document.querySelectorAll('.ts-ref-item')].find(e => e.textContent.includes('HeroType'));
  it.click();
  return true;
})()`);
await sleep(200);
check("引用类型预览", (await evalJs(`document.getElementById('ts-result').textContent`)) === "@HeroType");
await evalJs(`document.getElementById('ts-ok').click()`);
await sleep(300);
check("引用类型提交", (await evalJs(`__tt.state.snapshot.grid.headers[2][1]`)) === "@HeroType");

// 3.5 Map 参数位
await openHeader("2:1");
await until(`!document.getElementById('type-selector').hidden`, "第三次打开类型选择器");
// 当前值是 @HeroType，对话框默认落在「引用类型」tab；先切回数据类型。
await evalJs(`document.querySelector('[data-tstab=data]').click()`);
await sleep(150);
await evalJs(`(() => {
  [...document.querySelectorAll('.ts-paradigm')].find(e => e.textContent.includes('Map<_,_>')).click();
  return true;
})()`);
await sleep(250);
await evalJs(`(() => {
  const s = document.querySelector('[data-tsslot="1"]');
  s.value = 'str';
  s.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(300);
check("Map 预览", (await evalJs(`document.getElementById('ts-result').textContent`)) === "Map<int,str>");
await evalJs(`document.getElementById('ts-ok').click()`);
await sleep(300);
check(
  "Map 提交",
  (await evalJs(`__tt.state.snapshot.grid.headers[2][1]`)) === "Map<int,str>",
);

// 4. 引用显示：BuildingType 展示 desc，然后看 Building.kind
await evalJs(`(async () => {
  await __tt.runOp('select_node', { project: 'demo', kind: 'enum', group: 'enums', name: 'BuildingType' });
  await __tt.runOp('set_display_field', { field: 'desc' });
  await __tt.runOp('select_node', { project: 'demo', kind: 'table', group: 'building', name: 'Building' });
  await __tt.runOp('set_show_ref_name', { on: true });
  return true;
})()`);
const refValues = await evalJs(`__tt.state.snapshot.grid.display_rows.slice(0, 3).map(r => r[2])`);
check("引用显示展示值", JSON.stringify(refValues) === JSON.stringify(["资源", "军事", "特殊"]), refValues);
const refRaw = await evalJs(`__tt.state.snapshot.grid.rows.slice(0, 3).map(r => r[2])`);
check("引用列仍保留原始 id", JSON.stringify(refRaw) === JSON.stringify(["1", "2", "3"]), refRaw);

await evalJs(`__tt.runOp('set_show_ref_name', { on: false })`);
const rawValues = await evalJs(`__tt.state.snapshot.grid.rows.slice(0, 3).map(r => r[2])`);
check("关闭后回到原始 id", JSON.stringify(rawValues) === JSON.stringify(["1", "2", "3"]), rawValues);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
