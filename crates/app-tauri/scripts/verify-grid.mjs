// 表格交互自验：通过 WebView2 的 CDP 端口驱动真实页面。
// 前置：带 --remote-debugging-port=9222 启动 tablet-tauri，且工作区里至少有一个 table 节点。
// 用法：
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
//   tablet-tauri.exe --gui --workdir=<工作区>
//   node crates/app-tauri/scripts/verify-grid.mjs
import fs from "node:fs";

const PORT = process.env.CDP_PORT || 9222;
const OUT = process.env.SHOT_DIR || "target/tauri-shots";
fs.mkdirSync(OUT, { recursive: true });

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
  if (msg.error) throw new Error(JSON.stringify(msg.error));
  if (msg.result?.exceptionDetails) {
    throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
  }
  return msg.result?.result?.value;
}

async function shot(name) {
  const msg = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(`${OUT}/${name}`, Buffer.from(msg.result.data, "base64"));
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

await send("Runtime.enable");
await send("Page.enable");

// 0. 等首份快照
for (let i = 0; i < 80; i++) {
  const ready = await evalJs(`!!(window.__tt && __tt.state.snapshot)`);
  if (ready) break;
  await new Promise((r) => setTimeout(r, 250));
}

const tree = await evalJs(`__tt.state.snapshot.tree`);
const tableNode = tree.find((r) => r.kind === "table");
check("叶子节点存在", !!tableNode, tree.map((r) => `${r.kind}:${r.label}`));
if (!tableNode) {
  console.log("没有 table 节点，无法继续");
  process.exit(1);
}

// 1. 选中真实节点
const t1 = await evalJs(`(async () => {
  await __tt.runOp('select_node', {
    project: ${JSON.stringify(tableNode.project)},
    kind: ${JSON.stringify(tableNode.kind)},
    group: ${JSON.stringify(tableNode.group)},
    name: ${JSON.stringify(tableNode.name)},
  });
  return {
    title: document.getElementById('grid-title').textContent,
    rows: __tt.grid.rows,
    cols: __tt.grid.cols,
    dataCount: __tt.grid.node.dataCount,
  };
})()`);
check("真实节点加载", t1.rows > 0 && t1.cols > 0 && t1.title.includes(tableNode.name), t1);

// 2. 单格选择 + 公式栏
const t2 = await evalJs(`(() => {
  __tt.grid.selectCell(0, 1);
  return {
    info: __tt.grid.selectionInfo(),
    coord: document.getElementById('formula-coord').textContent,
    value: document.getElementById('formula-input').value,
    selected: document.querySelectorAll('td.cell-selected').length,
  };
})()`);
check("单格选择", t2.info === "B1" && t2.coord === "B1" && t2.selected === 1, t2);

// 3. 矩形选区
const t3 = await evalJs(`(() => {
  __tt.grid.selectCell(0, 0);
  __tt.grid.selectCell(1, 2, true);
  return {
    info: __tt.grid.selectionInfo(),
    painted: document.querySelectorAll('td.in-range, td.cell-selected').length,
  };
})()`);
check("矩形选区", t3.info.startsWith("A1:C2") && t3.painted === 6, t3);
await shot("tauri-grid-range.png");

// 4. 键盘移动
const t4 = await evalJs(`(() => {
  __tt.grid.selectCell(0, 0);
  const g = document.getElementById('grid-scroll');
  g.focus();
  g.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  return { r: __tt.grid.focus.r, c: __tt.grid.focus.c };
})()`);
check("方向键移动", t4.r === 1 && t4.c === 0, t4);

// 5. 编辑真实数据 → 后端回写
const marker = "自验-" + Date.now();
const t5 = await evalJs(`(async () => {
  const before = __tt.grid.cellText(0, 1);
  __tt.grid.selectCell(0, 1);
  __tt.grid.beginCellEdit(0, 1);
  const input = document.querySelector('td[data-cell="0:1"] input.cell-edit');
  if (!input) return { input: false };
  input.value = ${JSON.stringify(marker)};
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  await __tt.runOp('snapshot', {});
  return {
    input: true,
    before,
    local: __tt.grid.cellText(0, 1),
    backend: __tt.state.snapshot.grid.rows[0][1],
    dirty: __tt.state.snapshot.status.dirty,
  };
})()`);
check("编辑回写 core", t5.input && t5.backend === marker && t5.dirty, t5);

// 6. 撤销
const t6 = await evalJs(`(async () => {
  await __tt.runOp('undo', {});
  return { backend: __tt.state.snapshot.grid.rows[0][1], canUndo: __tt.state.snapshot.status.can_undo };
})()`);
check("撤销", t6.backend === t5.before, t6);

// 7. 表头编辑（desc 行）
const descMarker = "表头-" + Date.now();
const t7 = await evalJs(`(async () => {
  __tt.grid.beginHeaderEdit(0, 1);
  const th = document.querySelector('th[data-header="0:1"] input.cell-edit');
  if (!th) return { input: false };
  th.value = ${JSON.stringify(descMarker)};
  th.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  await __tt.runOp('snapshot', {});
  return { input: true, backend: __tt.state.snapshot.grid.headers[0][1] };
})()`);
check("表头编辑回写", t7.input && t7.backend === descMarker, t7);
await shot("tauri-grid-edit.png");

// 8. TSV 粘贴 + Delete
const t8 = await evalJs(`(async () => {
  __tt.grid.selectCell(0, 3);
  __tt.grid.pasteTSV('A\\tB');
  await __tt.runOp('snapshot', {});
  const pasted = [__tt.state.snapshot.grid.rows[0][3], __tt.state.snapshot.grid.rows[0][4]];
  __tt.grid.selectCell(0, 3);
  __tt.grid.selectCell(0, 4, true);
  __tt.grid.clearSelection();
  await __tt.runOp('snapshot', {});
  return { pasted, cleared: [__tt.state.snapshot.grid.rows[0][3], __tt.state.snapshot.grid.rows[0][4]] };
})()`);
check("粘贴 / 清空", t8.pasted[0] === "A" && t8.cleared[0] === "", t8);

// 9. 列宽
const t9 = await evalJs(`(() => {
  __tt.grid.setColWidth(1, 240);
  const col = document.querySelector('#sheet colgroup col:nth-child(3)');
  return { colWidth: col.style.width, model: __tt.grid.colWidths[1] };
})()`);
check("列宽调整", t9.colWidth === "240px" && t9.model === 240, t9);

// 10. 保存
const t10 = await evalJs(`(async () => {
  await __tt.runOp('save_workspace', {});
  return { dirty: __tt.state.snapshot.status.dirty };
})()`);
check("保存", t10.dirty === false, t10);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
