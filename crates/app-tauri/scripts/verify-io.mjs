// Schema / Excel / 问题 / 关于 / 条件构建器自验（CDP 驱动真实页面）。
// 需要 SHOT_DIR/工作区路径：WORKDIR=<工作区绝对路径>
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.CDP_PORT || 9222;
const WORKDIR = process.env.WORKDIR || "";

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
    schema: !document.getElementById('schema-dialog').hidden,
    excel: !document.getElementById('excel-transfer').hidden,
    map: !document.getElementById('excel-map').hidden,
    errors: [
      document.getElementById('schema-error').textContent,
      document.getElementById('excel-error').textContent,
      document.getElementById('em-error').textContent
    ],
    schemaResult: document.getElementById('schema-result').textContent,
    excelResult: document.getElementById('excel-result').textContent
  })`);
  console.log("DIAG", diag);
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

async function openProjectMenu(label) {
  await evalJs(`(() => {
    const el = document.querySelectorAll('.tree-item')[0];
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
  })()`);
  await sleep(200);
  await evalJs(`(() => {
    const el = [...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes(${JSON.stringify(label)}));
    if (!el) return false;
    el.click();
    return true;
  })()`);
}

async function openGroupMenu(group, label) {
  await evalJs(`(() => {
    const el = [...document.querySelectorAll('.tree-item')].find(e => e.dataset.key && e.dataset.key.startsWith('g:') && e.textContent.includes(${JSON.stringify(group)}));
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
  })()`);
  await sleep(200);
  await evalJs(`(() => {
    const el = [...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes(${JSON.stringify(label)}));
    if (!el) return false;
    el.click();
    return true;
  })()`);
}

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 1. Schema 导出（节点树 + metadata）
await openProjectMenu("导出 Schema");
await until(`!document.getElementById('schema-dialog').hidden`, "Schema 对话框");
await until(`document.querySelectorAll('#schema-out-tree .schema-node').length > 0`, "导出节点树");
const outTree = await evalJs(`({
  title: document.getElementById('schema-title').textContent,
  nodes: document.querySelectorAll('#schema-out-tree .schema-node').length,
  stat: document.getElementById('schema-out-stat').textContent
})`);
check("Schema 导出节点树", outTree.nodes >= 3 && outTree.stat.includes("已选"), outTree);
await evalJs(`(() => {
  document.getElementById('schema-meta-id').value = 'qa_demo';
  document.getElementById('schema-out').value = 'qa.tblschema';
  document.getElementById('schema-run').click();
  return true;
})()`);
await until(`document.getElementById('schema-result').textContent.includes('已导出')`, "Schema 导出");
const schemaFile = path.join(WORKDIR, "projects", "demo", "qa.tblschema");
check("Schema 文件生成", fs.existsSync(schemaFile), schemaFile);

// 2. Schema 导入（读取预览 → 冲突标记 → 导入）
await evalJs(`document.getElementById('schema-close').click()`);
await sleep(150);
await openProjectMenu("合并 Schema");
await until(`!document.getElementById('schema-dialog').hidden`, "Schema 导入对话框");
await evalJs(`(() => {
  const input = document.getElementById('schema-in');
  input.value = 'qa.tblschema';
  input.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await until(`document.querySelectorAll('#schema-in-tree .schema-node').length > 0`, "导入预览");
const inTree = await evalJs(`({
  conflicts: document.querySelectorAll('#schema-in-tree .schema-conflict').length,
  stat: document.getElementById('schema-in-stat').textContent
})`);
check("Schema 导入冲突标记", inTree.conflicts > 0 && inTree.stat.includes("冲突"), inTree);
await evalJs(`document.getElementById('schema-run').click()`);
await until(`document.getElementById('schema-result').textContent.includes('导入完成')`, "Schema 导入");
check("Schema 导入", true);
await evalJs(`document.getElementById('schema-close').click()`);

// 3. Excel 导出（多组对话框的后端命令；组/节点选择 UI 由全量回归覆盖）
const exported = await evalJs(`window.__TAURI__.core.invoke('excel_export_selected', {
  path: '.excel/hero.xlsx',
  selections: [{ group: 'hero', names: ['HeroBase'] }],
}).then(r => r).catch(e => 'ERR: ' + e)`);
const xlsx = path.join(WORKDIR, "projects", "demo", ".excel", "hero.xlsx");
check("Excel 文件生成", !String(exported).startsWith("ERR") && fs.existsSync(xlsx), exported);

// 6. 问题 tab
await evalJs(`document.querySelector('#bottom-tabs [data-tab="1"]').click()`);
await sleep(300);
const issues = await evalJs(`!!document.getElementById('issue-list')`);
check("问题面板", issues);

// 7. 关于
await evalJs(`document.getElementById('btn-about').click()`);
await sleep(200);
const about = await evalJs(`!document.getElementById('about-dialog').hidden && document.getElementById('about-rows').innerText.includes('版本')`);
check("关于对话框", about);
await evalJs(`document.getElementById('about-close').click()`);

// 8. 条件行与 when 编译
await evalJs(`(async () => {
  __tt.selectTree('table', 'demo', 'hero', 'HeroBase');
  await new Promise(r => setTimeout(r, 800));
  document.getElementById('btn-rules').click();
  await new Promise(r => setTimeout(r, 500));
  const t = [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'kind');
  if (t) t.click();
  document.getElementById('rules-add').click();
  return true;
})()`);
await sleep(300);
await evalJs(`(() => {
  const card = [...document.querySelectorAll('.rule-card')].pop();
  card.querySelector('.rrow.validator[data-selectable] [data-act=add-validator]').click();
  return true;
})()`);
await sleep(250);
await evalJs(`(() => {
  const rows = [...[...document.querySelectorAll('.rule-card')].pop().querySelectorAll('.rrow[data-selectable]')];
  rows[rows.length - 1].click();
  return true;
})()`);
await sleep(150);
await evalJs(`(() => {
  const sel = document.querySelector('[data-act="dfn"]');
  sel.value = 'c:eq';
  sel.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(250);
const builder = await evalJs(`(() => {
  const col = document.querySelector('[data-act="dcond-column"]');
  if (!col) return { ok: false };
  col.value = 'kind';
  col.dispatchEvent(new Event('change', { bubbles: true }));
  const op = document.querySelector('[data-act="dcond-op"]');
  op.value = 'eq';
  op.dispatchEvent(new Event('change', { bubbles: true }));
  return { ok: true };
})()`);
await sleep(200);
await evalJs(`(() => {
  const v = document.querySelector('[data-act="dcond-value"]');
  if (!v) return;
  v.value = '2';
  v.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
const condSummary = await evalJs(`(() => {
  const card = [...document.querySelectorAll('.rule-card')].pop();
  return card ? card.querySelector('.rule-summary').textContent : '';
})()`);
check("条件构建器", builder.ok && condSummary.includes("when(eq(kind, 2)"), condSummary);

// 再加一条条件：默认连接符 and -> when(and(...))
await evalJs(`(() => {
  const card = [...document.querySelectorAll('.rule-card')].pop();
  const row = card.querySelector('.rrow.cond[data-selectable]');
  row.querySelector('[data-act=add-validator]').click();
  return true;
})()`);
await sleep(250);
await evalJs(`(() => {
  const rows = [...[...document.querySelectorAll('.rule-card')].pop().querySelectorAll('.rrow[data-selectable]')];
  rows[rows.length - 1].click();
  return true;
})()`);
await sleep(150);
await evalJs(`(() => {
  const sel = document.querySelector('[data-act="dfn"]');
  sel.value = 'c:eq';
  sel.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(250);
await evalJs(`(() => {
  const col = document.querySelector('[data-act="dcond-column"]');
  if (!col) return;
  col.value = 'hp';
  col.dispatchEvent(new Event('change', { bubbles: true }));
  const v = document.querySelector('[data-act="dcond-value"]');
  if (v) {
    v.value = '0';
    v.dispatchEvent(new Event('input', { bubbles: true }));
  }
  return true;
})()`);
const nestedCond = await evalJs(`(() => {
  const card = [...document.querySelectorAll('.rule-card')].pop();
  return card ? card.querySelector('.rule-summary').textContent : '';
})()`);
check("条件 and 嵌套", nestedCond.includes("when(and("), nestedCond);
await evalJs(`document.querySelector('#rules-cancel')?.click()`);
await sleep(200);
await evalJs(`document.getElementById('modal-ok').click()`);

// 9. 错误定位跳转：把 Building.kind 改成不存在的枚举 id
await evalJs(`(async () => {
  await __tt.runOp('select_node', { project: 'demo', kind: 'table', group: 'building', name: 'Building' });
  await __tt.runOp('commit_cell', { row: 0, col: 2, value: '999' });
  return true;
})()`);
const issue = await evalJs(`__tt.state.snapshot.issues.find(i => i.group === 'building' && i.jump)`);
check("产生可定位问题", !!issue, issue);
if (issue) {
  await evalJs(`document.querySelector('#bottom-tabs [data-tab="1"]').click()`);
  await sleep(250);
  const idx = await evalJs(
    `__tt.state.issues.findIndex(i => i.group === 'building' && i.jump)`,
  );
  await evalJs(`document.querySelector('#issue-list [data-issue="${idx}"]').click()`);
  await sleep(600);
  const jumped = await evalJs(`({
    name: __tt.state.selected?.name,
    r: __tt.grid.focus.r,
    c: __tt.grid.focus.c
  })`);
  check(
    "问题跳转",
    jumped.name === "Building" && jumped.r === issue.row && jumped.c === issue.col,
    jumped,
  );
}

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
