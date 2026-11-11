// 剪贴板 TSV 自验：Excel 把含换行 / 引号的单元格加 `"` 包裹后粘贴，
// 不能把多行单元格拆成多行（多语言文案里常见，带 <div> 等标签）。
// 前置：demo 工作区（scripts/fixtures/demo.tblschema），选中 hero/HeroBase。
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

await send("Runtime.enable");
await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");
await evalJs(`__tt.selectTree('table', 'demo', 'hero', 'HeroBase')`);
await sleep(800);

// Excel 风格：行分隔 \r\n；含换行的单元格加引号，内部换行属于单元格内容；
// 引号本身用 "" 转义。
const tsv =
  'id\tname\tkind\tdesc\r\n' +
  '100\t"<div>第一行\r\n第二行</div>"\t战士\t"含 ""引号"" 的文本"\r\n' +
  '101\t普通\t法师\t结束\r\n' +
  '102\t第三行\t弓手\t收尾\r\n';

await evalJs(`(() => { __tt.grid.selectCell(0, 0, false); return true; })()`);
await sleep(200);
await evalJs(`__tt.grid.pasteTSV(${JSON.stringify(tsv)})`);
await sleep(900);

const pasted = await evalJs(`(() => {
  const g = __tt.state.snapshot.grid;
  return { count: g.data_count, rows: g.rows.slice(0, 4).map(r => r.slice(0, 4)) };
})()`);
check(
  "多行单元格不被拆行、后续行不前移",
  pasted.count === 4 &&
    JSON.stringify(pasted.rows[1]) === JSON.stringify(["100", "<div>第一行\n第二行</div>", "战士", '含 "引号" 的文本']) &&
    JSON.stringify(pasted.rows[2]) === JSON.stringify(["101", "普通", "法师", "结束"]) &&
    JSON.stringify(pasted.rows[3]) === JSON.stringify(["102", "第三行", "弓手", "收尾"]),
  pasted,
);

// HTML 标签必须按文本渲染，不能变成 DOM 节点（否则会打乱表格结构）。
const dom = await evalJs(`(() => {
  const td = document.querySelector('td[data-cell="1:1"]');
  return { children: td.children.length, text: td.textContent };
})()`);
check("HTML 标签按纯文本渲染", dom.children === 0 && dom.text.includes("<div>第一行\n第二行</div>"), dom);

// 复制回剪贴板要按同一套规则加引号，保证再次粘贴不丢结构。
const copied = await evalJs(`(() => {
  __tt.grid.selectCell(0, 0, false);
  __tt.grid.selectCell(2, 3, true);
  const tsv = __tt.grid.selectionTSV();
  const back = parseTSV(tsv);
  return { tsv, back };
})()`);
check(
  "复制 → 解析往返一致",
  copied.tsv.includes('"<div>第一行\n第二行</div>"') &&
    copied.tsv.includes('"含 ""引号"" 的文本"') &&
    JSON.stringify(copied.back) ===
      JSON.stringify([
        ["id", "name", "kind", "desc"],
        ["100", "<div>第一行\n第二行</div>", "战士", '含 "引号" 的文本'],
        ["101", "普通", "法师", "结束"],
      ]),
  copied,
);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
