// 引用选择器自验：枚举 / 表（智能·全部列展示）/ 列表引用多选。
// 需要带 HeroBase.equip(@Equip)、HeroBase.skills(List<@HeroType>)、Building.kind(@BuildingType)
// 的工作区：用 scripts/fixtures/demo.tblschema 建。
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
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

const openOn = async (node, cell) => {
  await evalJs(`(async () => {
    __tt.selectTree(${node.map((x) => JSON.stringify(x)).join(",")});
    await new Promise(r => setTimeout(r, 800));
    const td = document.querySelector('td[data-cell="${cell}"]');
    td.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    return true;
  })()`);
  await until(`!!document.querySelector('.ref-popup.ref-picker')`, "引用选择器");
};

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 1. 枚举引用：id/name/desc 三列 + 表头两行，无「列展示」
await openOn(["table", "demo", "building", "Building"], "0:2");
const enumPick = await evalJs(`({
  kind: document.querySelector('[data-ref-kind]').textContent,
  strategyHidden: document.querySelector('[data-ref-strategy]').hidden,
  desc: document.querySelector('.ref-popup thead .ref-desc')?.innerText || '',
  fields: document.querySelector('.ref-popup thead .ref-fields').innerText,
  rows: [...document.querySelectorAll('.ref-popup tbody tr')].map(r => r.innerText.replace(/\\s+/g, ' '))
})`);
check(
  "枚举引用表头 / 行",
  enumPick.kind === "枚举" &&
    enumPick.strategyHidden &&
    enumPick.fields.includes("name") &&
    enumPick.fields.includes("desc") &&
    enumPick.rows.length === 3 &&
    enumPick.rows[1].includes("MILITARY"),
  enumPick,
);
await evalJs(`(() => {
  const s = document.querySelector('[data-ref-search]');
  s.value = '军事';
  s.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()`);
const filtered = await evalJs(`[...document.querySelectorAll('.ref-popup tbody tr')].map(r => r.innerText.replace(/\\s+/g, ' '))`);
check("搜索任意列", filtered.length === 1 && filtered[0].includes("军事"), filtered);
await evalJs(`document.querySelector('.ref-popup tbody tr[data-id]').click()`);
await evalJs(`document.querySelector('[data-ref-ok]').click()`);
await sleep(300);
check("单击选中 + 确定提交", (await evalJs(`__tt.state.snapshot.grid.rows[0][2]`)) === "2");

// 2. 表引用：智能 / 全部列展示 + 双击提交
await openOn(["table", "demo", "hero", "HeroBase"], "0:6");
const smart = await evalJs(`({
  kind: document.querySelector('[data-ref-kind]').textContent,
  fields: document.querySelector('.ref-popup thead .ref-fields').innerText.replace(/\\s+/g, ' '),
  active: [...document.querySelectorAll('[data-strategy].primary')].map(b => b.dataset.strategy).join('')
})`);
check(
  "表引用智能列展示",
  smart.kind === "表" && smart.fields.includes("name") && smart.fields.includes("price") && smart.active === "auto",
  smart,
);
await evalJs(`document.querySelector('[data-strategy="full"]').click()`);
await sleep(500);
const full = await evalJs(`({
  fields: document.querySelector('.ref-popup thead .ref-fields').innerText.replace(/\\s+/g, ' '),
  active: [...document.querySelectorAll('[data-strategy].primary')].map(b => b.dataset.strategy).join('')
})`);
check("全部列展示", full.fields.includes("bonus") && full.active === "full", full);
await evalJs(`(async () => {
  const tb = document.querySelector('.ref-popup tbody');
  const hit = () => tb.querySelector('tr[data-id]').dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
  hit();
  await new Promise(r => setTimeout(r, 60));
  hit();
  return true;
})()`);
await sleep(300);
check("双击提交", (await evalJs(`__tt.state.snapshot.grid.rows[0][6]`)) === "2001");

// 3. 列表引用多选
await openOn(["table", "demo", "hero", "HeroBase"], "0:5");
const multi = await evalJs(`({
  checks: document.querySelectorAll('.ref-popup tbody .ref-check').length,
  preview: document.querySelector('[data-ref-preview]').textContent
})`);
check("列表引用多选", multi.checks === 3 && multi.preview.includes("已选 2 项"), multi);
await evalJs(`document.querySelectorAll('.ref-popup tbody tr[data-id]')[2].click()`);
const after = await evalJs(`({
  preview: document.querySelector('[data-ref-preview]').textContent,
  manual: document.querySelector('[data-ref-manual]').value
})`);
check("勾选同步当前值", after.manual === "1;2;3" && after.preview.includes("3 项"), after);
// 多选也支持双击：双击已勾选项 -> 保留全部勾选并提交。
await evalJs(`(async () => {
  const tb = document.querySelector('.ref-popup tbody');
  const hit = () => tb.querySelectorAll('tr[data-id]')[2].dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
  hit();
  await new Promise(r => setTimeout(r, 60));
  hit();
  return true;
})()`);
await sleep(300);
check("多选双击提交", (await evalJs(`__tt.state.snapshot.grid.rows[0][5]`)) === "1;2;3");
check("多选双击后关闭", (await evalJs(`!document.querySelector('.ref-popup')`)) === true);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
