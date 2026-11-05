// 校验规则编辑器自验（CDP 驱动真实页面，交互对齐 rule-builder-v2）。
// 前提：带 --remote-debugging-port=9222 启动 tablet-tauri，工作区含 hero/HeroBase。
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
    rulesOpen: !document.getElementById('rules').hidden,
    error: document.getElementById('rules-error').textContent,
    custom: document.getElementById('rules-custom').innerText.slice(0, 400)
  })`);
  console.log("DIAG", diag);
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

const lastCard = () => `[...document.querySelectorAll('.rule-card')].pop()`;

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 选中 hero/HeroBase 并打开规则编辑器
await evalJs(`(async () => {
  __tt.selectTree('table', 'demo', 'hero', 'HeroBase');
  await new Promise(r => setTimeout(r, 700));
  return true;
})()`);
await evalJs(`document.getElementById('btn-rules').click()`);
await until(`!document.getElementById('rules').hidden`, "规则编辑器打开");

const base = await evalJs(`({
  title: document.getElementById('rules-title').textContent,
  targets: [...document.querySelectorAll('.rule-target')].map(e => e.dataset.target),
  detail: !!document.getElementById('rule-detail'),
  status: document.getElementById('rules-status').innerText
})`);
check(
  "编辑器打开 / 目标列 / inspector",
  base.title.includes("HeroBase") && base.targets.includes("name") && base.detail,
  base,
);
check("状态条显示系统规则数", /系统规则/.test(base.status), base.status);

// 新建列规则 name：默认带一个验证器 non_empty，级别 warn
await evalJs(`(() => {
  [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'name').click();
  document.getElementById('rules-add').click();
  return true;
})()`);
await sleep(200);
await evalJs(`(() => {
  const level = ${lastCard()}.querySelector('.level');
  level.value = 'warn';
  level.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(150);
const built = await evalJs(`({
  summary: ${lastCard()}.querySelector('.rule-summary').textContent,
  row: ${lastCard()}.querySelector('.rrow.validator .row-fn')?.textContent || '',
  detail: document.getElementById('rule-detail').innerText
})`);
check(
  "默认验证器与摘要",
  built.summary.includes("column name") && built.summary.includes("non_empty") && built.row.includes("non_empty"),
  built,
);
check(
  "inspector 显示说明 / 示例",
  built.detail.includes("说明") && built.detail.includes("# @rule column name"),
  built.detail.slice(0, 200),
);

// 加一条函数行并把函数换成条件 eq(kind, 2)
await evalJs(`(() => {
  const row = ${lastCard()}.querySelector('.rrow.validator[data-selectable]');
  row.querySelector('[data-act="add-validator"]').click();
  return true;
})()`);
await sleep(200);
await evalJs(`(() => {
  const rows = [...${lastCard()}.querySelectorAll('.rrow[data-selectable]')];
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
  col.value = 'kind';
  col.dispatchEvent(new Event('change', { bubbles: true }));
  const op = document.querySelector('[data-act="dcond-op"]');
  op.value = 'eq';
  op.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`);
await sleep(200);
await evalJs(`(() => {
  const v = document.querySelector('[data-act="dcond-value"]');
  v.value = '2';
  v.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
await sleep(150);
const withCond = await evalJs(`({
  summary: ${lastCard()}.querySelector('.rule-summary').textContent,
  rows: [...${lastCard()}.querySelectorAll('.rrow[data-selectable]')].map(r => r.className + '|' + r.querySelector('.row-fn').textContent)
})`);
check(
  "条件行与 when 编译",
  withCond.summary.includes("when(eq(kind, 2)") &&
    withCond.rows.length === 2 &&
    withCond.rows.some((r) => r.includes("cond")),
  withCond,
);

// 保存列规则
await evalJs(`document.getElementById('rules-save').click()`);
await until(`document.getElementById('rules').hidden`, "保存列规则");
const saved = await evalJs(`(async () => {
  const p = await window.__TAURI__.core.invoke('rules_payload', { project: 'demo', group: 'hero', name: 'HeroBase' });
  return p.rules;
})()`);
check(
  "列规则落库（When + 验证器）",
  saved.length === 1 &&
    saved[0].target === "name" &&
    saved[0].level === "warn" &&
    saved[0].expr.kind === "When" &&
    saved[0].expr.cond === "eq(kind, 2)" &&
    saved[0].expr.inner?.name === "non_empty",
  saved,
);

// 表级规则：默认 row_count(0, ) -> 填 1 / 100
await evalJs(`document.getElementById('btn-rules').click()`);
await until(`!document.getElementById('rules').hidden`, "再次打开");
await evalJs(`(() => {
  document.querySelector('[data-scope="table"]').click();
  document.getElementById('rules-add').click();
  return true;
})()`);
await sleep(200);
const tableDefault = await evalJs(`${lastCard()}.querySelector('.rule-summary').textContent`);
check("表级默认 row_count", tableDefault.includes("row_count("), tableDefault);
await evalJs(`(() => {
  const ins = [...document.querySelectorAll('#rule-detail [data-act="dparam"]')];
  ins[0].value = '1';
  ins[0].dispatchEvent(new Event('input', { bubbles: true }));
  ins[1].value = '100';
  ins[1].dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
await sleep(150);
await evalJs(`document.getElementById('rules-save').click()`);
await until(`document.getElementById('rules').hidden`, "保存表级规则");
const tableSaved = await evalJs(`(async () => {
  const p = await window.__TAURI__.core.invoke('rules_payload', { project: 'demo', group: 'hero', name: 'HeroBase' });
  return p.rules;
})()`);
const tableRule = tableSaved.find((r) => r.target === "");
check(
  "表级规则落库",
  tableRule?.expr?.name === "row_count" && (tableRule?.expr?.args || []).join(",") === "1,100",
  tableRule,
);

// 系统规则只读
await evalJs(`document.getElementById('btn-rules').click()`);
await until(`!document.getElementById('rules').hidden`, "第三次打开");
await evalJs(`(() => {
  [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'id').click();
  document.querySelector('[data-rtab="system"]').click();
  return true;
})()`);
const sys = await evalJs(`({
  rows: document.querySelectorAll('#rules-system .sys-row').length,
  buttons: document.querySelectorAll('#rules-system button').length
})`);
check("系统规则只读", sys.rows > 0 && sys.buttons === 0, sys);

// 还原：改一条规则后点「还原」应回到已保存状态
await evalJs(`(() => {
  document.querySelector('[data-rtab="custom"]').click();
  [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'name').click();
  document.getElementById('rules-add').click();
  return true;
})()`);
await sleep(150);
await evalJs(`document.getElementById('rules-reset').click()`);
await until(`!document.getElementById('modal').hidden`, "还原确认");
await evalJs(`document.getElementById('modal-ok').click()`);
await sleep(300);
const afterReset = await evalJs(`({
  cards: document.querySelectorAll('.rule-card').length,
  dirty: document.getElementById('rules-status').innerText.includes('未保存')
})`);
check("还原丢弃未保存规则", afterReset.cards === 1 && !afterReset.dirty, afterReset);

// 切换列：有未保存改动时先显示内联确认条
await evalJs(`(() => {
  document.getElementById('rules-add').click();
  [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'hp').click();
  return true;
})()`);
await sleep(150);
const bar = await evalJs(`({
  shown: !document.getElementById('rules-discard').hidden,
  msg: document.getElementById('rules-discard-msg').textContent
})`);
check("切换列显示丢弃确认条", bar.shown && bar.msg.includes("hp"), bar);
await evalJs(`document.getElementById('rules-discard-cancel').click()`);
await sleep(120);
const cancelled = await evalJs(`({
  bar: !document.getElementById('rules-discard').hidden,
  active: [...document.querySelectorAll('.rule-target.active')].map(e => e.dataset.target),
  cards: document.querySelectorAll('.rule-card').length
})`);
check(
  "取消后留在原列且草稿保留",
  !cancelled.bar && cancelled.active.includes("name") && cancelled.cards === 2,
  cancelled,
);
await evalJs(`(() => {
  [...document.querySelectorAll('.rule-target')].find(e => e.dataset.target === 'hp').click();
  return true;
})()`);
await sleep(120);
await evalJs(`document.getElementById('rules-discard-ok').click()`);
await sleep(250);
const switched = await evalJs(`({
  active: [...document.querySelectorAll('.rule-target.active')].map(e => e.dataset.target),
  dirty: document.getElementById('rules-status').innerText.includes('未保存'),
  cards: document.querySelectorAll('.rule-card').length
})`);
check(
  "丢弃并切换",
  switched.active.includes("hp") && !switched.dirty && switched.cards === 0,
  switched,
);

// 未保存关闭 → 丢弃确认；取消后仍开着，确认后关闭
await evalJs(`(() => {
  document.getElementById('rules-add').click();
  document.getElementById('rules-cancel').click();
  return true;
})()`);
await until(`!document.getElementById('modal').hidden`, "丢弃确认弹窗");
await evalJs(`document.getElementById('modal-cancel').click()`);
await sleep(150);
const stillOpen = await evalJs(`!document.getElementById('rules').hidden`);
check("取消丢弃后保持打开", stillOpen, { stillOpen });

await evalJs(`document.getElementById('rules-cancel').click()`);
await sleep(150);
await evalJs(`document.getElementById('modal-ok').click()`);
await sleep(150);
const closed = await evalJs(`document.getElementById('rules').hidden`);
check("确认丢弃后关闭", closed, { closed });

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
