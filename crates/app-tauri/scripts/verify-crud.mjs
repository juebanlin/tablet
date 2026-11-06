// 右键菜单 / 节点增删改自验（CDP 驱动真实页面）。
// 前提：带 --remote-debugging-port=9222 启动 tablet-tauri，工作区里至少有一个 project。
// 用法：node crates/app-tauri/scripts/verify-crud.mjs
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
    tree: __tt.state.snapshot.tree.map(r => r.kind + ':' + r.label + ':' + r.mark),
    logs: __tt.state.snapshot.logs.slice(0, 8).map(l => l.level + ' ' + l.message),
    modalOpen: !document.getElementById('modal').hidden,
    menuOpen: !document.getElementById('ctx-menu').hidden,
    menuText: document.getElementById('ctx-menu').innerText,
    modalTitle: document.getElementById('modal-title').textContent
  })`);
  console.log("DIAG", diag);
  throw new Error(`超时: ${label}`);
}

async function contextMenu(expr, label) {
  const ok = await evalJs(`(() => {
    const el = ${expr};
    if (!el) return false;
    el.scrollIntoView({ block: 'center' });
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent('contextmenu', {
      bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8
    }));
    return true;
  })()`);
  if (!ok) throw new Error(`找不到右键目标: ${label}`);
}

async function menuClick(label) {
  const ok = await evalJs(`(() => {
    const el = [...document.querySelectorAll('.ctx-item')].find(e => (e.textContent || '').includes(${JSON.stringify(label)}));
    if (!el) return false;
    el.click();
    return true;
  })()`);
  if (!ok) throw new Error(`找不到菜单项: ${label}`);
}

async function modalInput(value) {
  await sleep(120);
  await evalJs(`(() => {
    const i = document.getElementById('modal-input');
    if (!i || document.getElementById('modal').hidden) return false;
    i.value = ${JSON.stringify(value)};
    // 命名校验是实时的（core 规则）：「确定」在错误清除前是禁用的，像真人一样先触发 input。
    i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  })()`);
  await sleep(250);
  await evalJs(`document.getElementById('modal-ok').click()`);
}

async function modalOk() {
  await sleep(120);
  await evalJs(`document.getElementById('modal-ok').click()`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

const project = await evalJs(`__tt.state.snapshot.tree.find(r => r.kind === 'project' && !r.closed)?.project`);
check("找到已打开项目", !!project, project);
if (!project) process.exit(1);

// 1. 项目右键 → 新建 Group
await contextMenu(`document.querySelector('.tree-item[data-key^="p:"]')`, "项目行");
await menuClick("新建 Group");
await modalInput("qa_group");
await until(`__tt.state.snapshot.tree.some(r => r.kind === 'group' && r.label === 'qa_group')`, "新建 Group");
check("新建 Group", true);

// 2. Group 右键 → 新建 Table
await contextMenu(
  `[...document.querySelectorAll('.tree-item')].find(e => e.dataset.key.startsWith('g:') && e.textContent.includes('qa_group'))`,
  "group qa_group",
);
await menuClick("新建 Table");
await modalInput("QaTable");
await until(`__tt.state.snapshot.tree.some(r => r.kind === 'table' && r.label === 'QaTable')`, "新建 Table");
check("新建 Table", true);

// 3. 节点右键 → 重命名
const nodeExpr = (name) =>
  `[...document.querySelectorAll('.tree-item')].find(e => e.dataset.key.startsWith('n:') && e.textContent.includes('${name}'))`;
await contextMenu(nodeExpr("QaTable"), "QaTable");
await menuClick("重命名");
await modalInput("QaTable2");
await until(`__tt.state.snapshot.tree.some(r => r.kind === 'table' && r.label === 'QaTable2')`, "重命名节点");
check("重命名节点", true);

// 4. 复制 → 粘贴到同一 Group（自动加 _copy 后缀）
await contextMenu(nodeExpr("QaTable2"), "QaTable2");
await menuClick("复制");
await sleep(350);
await contextMenu(
  `[...document.querySelectorAll('.tree-item')].find(e => e.dataset.key.startsWith('g:') && e.textContent.includes('qa_group'))`,
  "group qa_group",
);
await menuClick("粘贴节点");
await until(`__tt.state.snapshot.tree.some(r => r.label.startsWith('QaTable2_copy'))`, "粘贴节点");
check("复制 / 粘贴节点", true);

// 5. 删除节点（软删）
const copied = await evalJs(`__tt.state.snapshot.tree.find(r => r.label.startsWith('QaTable2_copy'))?.name`);
await contextMenu(nodeExpr(copied), copied);
await menuClick("删除");
await modalOk();
await until(
  `__tt.state.snapshot.tree.some(r => r.name === ${JSON.stringify(copied)} && r.mark === '-')`,
  "删除节点",
);
check("删除节点", true);

// 6. 表格右键 → 删除行
await evalJs(`__tt.runOp('select_node', { project: ${JSON.stringify(project)}, kind: 'table', group: 'building', name: 'Building' })`);
await evalJs(`__tt.grid.selectCell(0, 0)`);
const before = await evalJs(`__tt.state.snapshot.grid.data_count`);
await contextMenu(`document.querySelector('td[data-cell="0:0"]')`, "单元格 0:0");
await menuClick("删除行");
await until(`__tt.state.snapshot.grid.data_count === ${before - 1}`, "删除行");
check("右键删除行", true, { before, after: before - 1 });

// 7. 撤销恢复
await evalJs(`__tt.runOp('undo', {})`);
await until(`__tt.state.snapshot.grid.data_count === ${before}`, "撤销删除行");
check("撤销行删除", true);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
