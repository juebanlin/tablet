// 依赖分析视图自验：入口 / 节点边 / 角色详情 / 搜索 / 滚轮缩放 / 左键平移。
// 前置：带 --remote-debugging-port 启动 tablet，工作区至少有一个打开的项目。
//   CDP_PORT=9444 node crates/app-tauri/scripts/verify-dependency.mjs
const PORT = process.env.CDP_PORT || 9222;

const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const page = targets.find((t) => t.type === "page" && t.url?.includes("tauri.localhost"));
if (!page) throw new Error("没有找到 tablet 页面 target");

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let seq = 0;
const pending = new Map();
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
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
  if (msg.result?.exceptionDetails) throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
  return msg.result?.result?.value;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(expr, label, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await evalJs(expr)) return true;
    await sleep(120);
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
await evalJs(`document.getElementById('btn-dependency').click()`);
await until(`!document.getElementById('dependency-overlay').hidden`, "依赖图打开");
await until(`document.querySelectorAll('#dep-nodes [data-node]').length > 0`, "节点渲染");
await sleep(300);

const base = await evalJs(`({
  status: document.getElementById('dep-status').textContent,
  nodes: document.querySelectorAll('#dep-nodes [data-node]').length,
  edges: document.querySelectorAll('#dep-edges path[data-edge]').length,
  sidebar: document.querySelectorAll('#dep-node-list [data-side]').length,
  zoom: document.getElementById('dep-zoom').textContent
})`);
check("打开后节点 / 边 / 侧栏", base.nodes > 0 && base.edges > 0 && base.sidebar > 0, base);

const firstNode = await evalJs(`document.querySelector('#dep-nodes [data-node]')?.dataset.node || ''`);
await evalJs(`document.querySelector('#dep-nodes [data-node="${firstNode}"]')?.click()`);
check("点击节点更新详情", (await evalJs(`document.querySelector('#dep-inspector .dep-ins-title')?.textContent || ''`)) === firstNode);

const oldCount = await evalJs(`document.querySelectorAll('#dep-nodes [data-node]:not([hidden])').length`);
await evalJs(`(() => { const i=document.getElementById('dep-search'); i.value='__no_such_node__'; i.dispatchEvent(new Event('input',{bubbles:true})); })()`);
await sleep(100);
const filtered = await evalJs(`document.querySelectorAll('#dep-nodes [data-node]:not([hidden])').length`);
check("搜索过滤节点", filtered === 0, { before: oldCount, after: filtered });
await evalJs(`(() => { const i=document.getElementById('dep-search'); i.value=''; i.dispatchEvent(new Event('input',{bubbles:true})); })()`);

const z0 = await evalJs(`document.getElementById('dep-zoom').textContent`);
await evalJs(`(() => { const s=document.getElementById('dep-scroll'); s.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,bubbles:true,cancelable:true,clientX:500,clientY:400})); })()`);
const z1 = await evalJs(`document.getElementById('dep-zoom').textContent`);
check("滚轮缩放", z0 !== z1, { before: z0, after: z1 });

await evalJs(`document.getElementById('dep-zoom-in').click()`);
await evalJs(`document.getElementById('dep-zoom-in').click()`);
const beforePan = await evalJs(`[document.getElementById('dep-scroll').scrollLeft, document.getElementById('dep-scroll').scrollTop]`);
await evalJs(`(() => { const s=document.getElementById('dep-scroll'); const b={bubbles:true,cancelable:true,pointerId:1,button:0,pointerType:'mouse',isPrimary:true}; s.dispatchEvent(new PointerEvent('pointerdown',{...b,clientX:500,clientY:400})); s.dispatchEvent(new PointerEvent('pointermove',{...b,clientX:410,clientY:330})); s.dispatchEvent(new PointerEvent('pointerup',{...b,clientX:410,clientY:330})); })()`);
const afterPan = await evalJs(`[document.getElementById('dep-scroll').scrollLeft, document.getElementById('dep-scroll').scrollTop]`);
check("左键拖动平移", beforePan[0] !== afterPan[0] || beforePan[1] !== afterPan[1], { beforePan, afterPan });

await evalJs(`document.getElementById('dep-close').click()`);
check("关闭依赖图", (await evalJs(`document.getElementById('dependency-overlay').hidden`)) === true);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
