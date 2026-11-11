// 主动验证日志自验：明细进 UI 日志 + tablet.log；重复验证先清掉上一块。
// 前置：demo 工作区（scripts/fixtures/demo.tblschema），HeroBase.hp 为 int。
// 用 WORKDIR=<工作区绝对路径> 时额外检查日志文件。
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

const commitHp = async (value) => {
  await evalJs(`(async () => {
    const s = await window.__TAURI__.core.invoke('commit_cell', { row: 0, col: 3, value: ${JSON.stringify(value)} });
    __tt.applySnapshot(s);
    return true;
  })()`);
  await sleep(400);
};
const validate = async () => {
  await evalJs(`__tt.runOp('validate_workspace', {})`);
  await sleep(500);
  return evalJs(
    `__tt.state.snapshot.logs.filter(l => l.message.includes('验证')).map(l => [l.level, l.message])`,
  );
};

// 1. 制造一个 int 错误 → 主动验证应把明细写进 UI 日志
await commitHp("abc");
const first = await validate();
check(
  "验证明细进 UI 日志",
  first.some(([level, msg]) => level === "ERROR" && msg.includes("hero/HeroBase") &&
    msg.includes("abc") && msg.includes("不是合法int")) &&
    first.some(([, msg]) => msg.includes("验证完成")),
  first,
);

// 2. 再点一次「重新验证」→ 上一块验证日志先被清掉，不重复堆叠
const second = await validate();
check(
  "重复验证不堆叠旧日志",
  second.filter(([, msg]) => msg.startsWith("[验证] 项目")).length === 1 &&
    second.filter(([, msg]) => msg.includes("验证完成")).length === 1 &&
    second.filter(([, msg]) => msg.includes("不是合法int")).length === 1,
  second,
);

// 3. 修回合法值 → 只剩「验证通过」，旧错误块也被清掉
await commitHp("1200");
const third = await validate();
check(
  "修复后旧块清空",
  third.length === 1 && third[0][1].includes("验证通过"),
  third,
);

// 4. 日志文件：明细按追加语义落盘（可选，需 WORKDIR）
if (WORKDIR) {
  const fs = await import("node:fs");
  const path = await import("node:path");
  const log = path.join(WORKDIR, "tablet.log");
  const text = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "";
  check(
    "日志文件含验证明细",
    text.includes("[验证] hero/HeroBase") && text.includes("不是合法int") && text.includes("验证通过"),
    { log, tail: text.split("\n").slice(-6) },
  );
}

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
