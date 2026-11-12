// 新建项目（旧版 布局：空 / 文件 / 模板 + 段选择）与项目设置自验。
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
  const diag = await evalJs(`JSON.stringify({
    dialog: !document.getElementById('create-project').hidden,
    error: document.getElementById('cp-error').textContent,
    tabs: [...document.querySelectorAll('#cp-tabs .ps-tab')].map(t => t.textContent),
    templateItems: document.querySelectorAll('#cp-tpl-list .cp-tpl').length,
    sections: document.querySelectorAll('#cp-tpl-sections .cp-sec').length
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

// 1. 对话框三来源
await evalJs(`document.getElementById('btn-new-project').click()`);
await until(`!document.getElementById('create-project').hidden`, "新建项目对话框");
const tabs = await evalJs(`[...document.querySelectorAll('#cp-tabs .ps-tab')].map(t => t.textContent.trim())`);
check("三来源 tab", tabs.join(",") === "空项目,从文件…,从模板", tabs);

// 2. 空项目
await evalJs(`(() => {
  document.querySelector('[data-cptab="empty"]').click();
  document.getElementById('cp-id').value = 'qa_empty';
  document.getElementById('cp-name').value = '自验空项目';
  document.getElementById('cp-category').value = 'qa';
  document.getElementById('cp-version').value = '1.0';
  document.getElementById('cp-ok').click();
  return true;
})()`);
await until(`__tt.state.snapshot.tree.some(r => r.kind === 'project' && r.project === 'qa_empty')`, "空项目创建");
check("空项目创建", true);

// 3. 模板项目（列表 → 段选择 → 创建）
await evalJs(`document.getElementById('btn-new-project').click()`);
await until(`!document.getElementById('create-project').hidden`, "新建项目对话框(2)");
await evalJs(`document.querySelector('[data-cptab="template"]').click()`);
await until(`document.querySelectorAll('#cp-tpl-list .cp-tpl').length > 0`, "模板列表");
await evalJs(`document.querySelector('#cp-tpl-list .cp-tpl').click()`);
await until(`document.querySelectorAll('#cp-tpl-sections .cp-sec').length > 0`, "模板段列表");
const sections = await evalJs(`document.querySelectorAll('#cp-tpl-sections .cp-sec').length`);
check("模板段选择", sections > 0, { sections });
await evalJs(`(() => {
  document.getElementById('cp-id').value = 'qa_tpl';
  document.getElementById('cp-name').value = '自验模板项目';
  document.getElementById('cp-ok').click();
  return true;
})()`);
await until(`__tt.state.snapshot.tree.some(r => r.kind === 'project' && r.project === 'qa_tpl')`, "模板项目创建");
const tplNodes = await evalJs(`__tt.state.snapshot.tree.filter(r => r.project === 'qa_tpl' && ['table','constant','enum'].includes(r.kind)).length`);
check("模板项目带节点", tplNodes > 0, { nodes: tplNodes });

// 4. 项目设置
await evalJs(`(() => {
  const el = [...document.querySelectorAll('.tree-item')].find(e => e.dataset.key === 'p:qa_tpl');
  const r = el.getBoundingClientRect();
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
})()`);
await sleep(200);
await evalJs(`[...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes('项目设置')).click()`);
await until(`!document.getElementById('project-settings').hidden`, "项目设置");
await evalJs(`(() => {
  document.getElementById('ps-name').value = '自验项目改名';
  document.getElementById('ps-ok').click();
  return true;
})()`);
await until(`__tt.state.snapshot.tree.some(r => r.project === 'qa_tpl' && r.label === '自验项目改名')`, "项目设置保存");
check("项目设置保存", true);

// 5. 分别保存两个新项目
await evalJs(`(async () => {
  await __tt.runOp('select_node', { project: 'qa_tpl', kind: 'project' });
  await __tt.runOp('save_workspace', {});
  await __tt.runOp('select_node', { project: 'qa_empty', kind: 'project' });
  await __tt.runOp('save_workspace', {});
  return true;
})()`);
await until(`__tt.state.snapshot.status.dirty === false`, "保存工作区");
check("保存工作区", true);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
