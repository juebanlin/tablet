// 列表引用分隔符自验：项目 `[separators].list` 改成 `#` 后，
// 选择器拼值 / 拆分现有值 / 引用名展示都必须跟随项目分隔符，不能写死 `;`。
// 前提：demo 工作区（scripts/fixtures/demo.tblschema），HeroBase.skills = List<@HeroType>。
// 脚本通过项目设置对话框改分隔符，结束时还原成默认 `;`。
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

/** 打开项目设置 → 分隔符 tab → 把 List 改成 value → 保存。 */
async function setListSeparator(value) {
  await evalJs(`(() => {
    const el = document.querySelector('.tree-item[data-key^="p:"]') || document.querySelectorAll('.tree-item')[0];
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 8 }));
  })()`);
  await sleep(200);
  await evalJs(`[...document.querySelectorAll('.ctx-item')].find(e => e.textContent.includes('项目设置')).click()`);
  await until(`!document.getElementById('project-settings').hidden`, "项目设置打开");
  await evalJs(`(() => {
    document.querySelector('[data-pstab=sep]').click();
    const input = document.querySelector('#ps-seps input[data-sep-key="List"]');
    input.value = ${JSON.stringify(value)};
    input.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  })()`);
  await evalJs(`document.getElementById('ps-ok').click()`);
  await until(`document.getElementById('project-settings').hidden`, "保存项目设置");
  await sleep(300);
}

const openPickerOnSkills = async () => {
  await evalJs(`(() => {
    const td = document.querySelector('td[data-cell="0:5"]');
    td.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    return true;
  })()`);
  await until(`!!document.querySelector('.ref-popup.ref-picker')`, "引用选择器");
};

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");

// 1. 项目分隔符改成 #，后端投影必须跟着变
await setListSeparator("#");
await evalJs(`__tt.selectTree("table", "demo", "hero", "HeroBase")`);
await sleep(900);
const seps = await evalJs(`({
  ref_seps: __tt.state.snapshot.grid.ref_seps,
  ref_list: __tt.state.snapshot.grid.ref_list
})`);
check("后端下发列表分隔符", seps.ref_seps[5] === "#" && seps.ref_list[5] === true, seps);

// 2. 单元格里的旧值 1;2 按 # 拆不开 → 无勾选，当前值原样保留
await evalJs(`__tt.grid.setCellValue(0, 5, "1;2")`);
await sleep(600);
await openPickerOnSkills();
const before = await evalJs(`({
  manual: document.querySelector('[data-ref-manual]').value,
  checks: document.querySelectorAll('.ref-popup tbody .ref-check.selected').length
})`);
check("按 # 拆分现有值", before.manual === "1;2" && before.checks === 0, before);

// 3. 清空后多选三项 → 拼成 1#2#3
await evalJs(`document.querySelector('[data-ref-clear]').click()`);
await sleep(300);
await openPickerOnSkills();
await evalJs(`document.querySelectorAll('.ref-popup tbody tr[data-id]')[0].click()`);
await evalJs(`document.querySelectorAll('.ref-popup tbody tr[data-id]')[1].click()`);
await evalJs(`document.querySelectorAll('.ref-popup tbody tr[data-id]')[2].click()`);
const picked = await evalJs(`({
  manual: document.querySelector('[data-ref-manual]').value,
  preview: document.querySelector('[data-ref-preview]').textContent
})`);
check("多选拼项目分隔符", picked.manual === "1#2#3" && picked.preview.includes("1#2#3"), picked);
await evalJs(`document.querySelector('[data-ref-ok]').click()`);
await sleep(400);
const committed = await evalJs(`__tt.state.snapshot.grid.rows[0][5]`);
check("提交值使用项目分隔符", committed === "1#2#3", committed);

// 4. 引用名展示：拆分 / 连接同样用 #（HeroType 的 @display 是 name）
await evalJs(`__tt.runOp('set_show_ref_name', { on: true })`);
await sleep(500);
const display = await evalJs(`__tt.state.snapshot.grid.display_rows[0][5]`);
check("引用名展示按项目分隔符拼接", display === "WARRIOR#MAGE#ARCHER", display);
await evalJs(`__tt.runOp('set_show_ref_name', { on: false })`);
await sleep(300);

// 5. 还原：数据恢复成默认分隔符可读的形式，再把项目分隔符改回 ;
await evalJs(`__tt.grid.setCellValue(0, 5, "1;2")`);
await sleep(600);
await setListSeparator(";");
// 右键项目行会切到项目节点（grid 变空），重新选中表格再读。
await evalJs(`__tt.selectTree("table", "demo", "hero", "HeroBase")`);
await sleep(900);
const restored = await evalJs(`__tt.state.snapshot.grid.ref_seps[5]`);
check("分隔符还原", restored !== "#", restored);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
