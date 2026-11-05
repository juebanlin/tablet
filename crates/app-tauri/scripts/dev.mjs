// Tauri 版开发/调试工具：通过 WebView2 的 CDP 端口操作真实页面。
//
// 前提：WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 启动 tablet-tauri。
//
// 用法：
//   node crates/app-tauri/scripts/dev.mjs shot [文件名]
//   node crates/app-tauri/scripts/dev.mjs eval "<js 表达式>"
//   node crates/app-tauri/scripts/dev.mjs click "<CSS 选择器>"
//   node crates/app-tauri/scripts/dev.mjs key "<按键>"
//   node crates/app-tauri/scripts/dev.mjs type "<CSS 选择器>" "<文本>"
//   node crates/app-tauri/scripts/dev.mjs snap
//
// 输出统一是 JSON，便于其它脚本 / AI 直接消费。截图默认落在 target/tauri-shots/。
import fs from "node:fs";

const PORT = process.env.CDP_PORT || 9222;
const OUT = process.env.SHOT_DIR || "target/tauri-shots";
fs.mkdirSync(OUT, { recursive: true });

const [cmd, ...args] = process.argv.slice(2);
if (!cmd) {
  console.error("用法见文件头注释");
  process.exit(2);
}

const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) {
  console.error("没有找到页面 target，确认已带 --remote-debugging-port=9222 启动");
  process.exit(1);
}

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

async function shot(name) {
  const file = `${OUT}/${name || `shot-${Date.now()}.png`}`;
  const msg = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(file, Buffer.from(msg.result.data, "base64"));
  return file;
}

async function click(selector) {
  return clickExpr(`document.querySelector(${JSON.stringify(selector)})`, selector);
}

/** 按可见文本点击（PowerShell 会吃掉选择器里的引号，文本匹配更稳）。 */
async function clickText(text, scope) {
  const expr = `(() => {
    const root = ${scope ? `document.querySelector(${JSON.stringify(scope)})` : "document"};
    if (!root) return null;
    const els = [...root.querySelectorAll('button, .tree-item, .tab, td, th, .chip, .chip-btn, a')];
    return els.find(e => (e.textContent || '').trim().includes(${JSON.stringify(text)})) || null;
  })()`;
  return clickExpr(expr, `text=${text}`);
}

async function clickExpr(expr, label) {
  const js = `(() => {
    const el = ${expr};
    if (!el) return { ok: false, reason: 'not found', selector: ${JSON.stringify(label)} };
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const base = { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, pointerId: 1 };
    el.dispatchEvent(new PointerEvent('pointerdown', { ...base, pointerType: 'mouse', isPrimary: true }));
    document.dispatchEvent(new PointerEvent('pointerup', { ...base, pointerType: 'mouse', isPrimary: true }));
    el.dispatchEvent(new MouseEvent('click', { ...base, detail: 1 }));
    return { ok: true, tag: el.tagName, text: (el.textContent || '').slice(0, 40) };
  })()`;
  return evalJs(js);
}

async function key(k) {
  const expr = `(() => {
    const el = document.activeElement || document.body;
    el.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(k)}, bubbles: true }));
    return { ok: true, target: el.id || el.tagName };
  })()`;
  return evalJs(expr);
}

async function typeInto(selector, text) {
  const expr = `(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    if (!el) return { ok: false, reason: 'not found' };
    el.focus();
    el.value = ${JSON.stringify(text)};
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: true, value: el.value };
  })()`;
  return evalJs(expr);
}

async function snap() {
  const expr = `(() => {
    const s = window.__tt?.state?.snapshot;
    if (!s) return { ready: false };
    return {
      ready: true,
      version: s.version,
      vcs: s.vcs,
      selected: window.__tt.state.selected,
      tree: s.tree.map(r => ({ kind: r.kind, label: r.label, mark: r.mark, project: r.project, group: r.group, name: r.name, closed: r.closed })),
      grid: s.grid ? { title: s.grid.title, rows: s.grid.rows.length, cols: s.grid.headers[0].length, dataCount: s.grid.data_count, errors: s.grid.errors.length } : null,
      status: s.status,
    };
  })()`;
  return evalJs(expr);
}

async function setTheme(theme) {
  const value = theme === "dark" ? "dark" : "light";
  await evalJs(`document.documentElement.dataset.theme = ${JSON.stringify(value)}; true`);
  return { ok: true, theme: value };
}

async function drag(fromSelector, toSelector) {
  const rects = await evalJs(`(() => {
    const a = document.querySelector(${JSON.stringify(fromSelector)});
    const b = document.querySelector(${JSON.stringify(toSelector)});
    if (!a || !b) return null;
    const ra = a.getBoundingClientRect();
    const rb = b.getBoundingClientRect();
    return {
      x1: ra.left + ra.width / 2, y1: ra.top + ra.height / 2,
      x2: rb.left + rb.width / 2, y2: rb.top + rb.height / 2
    };
  })()`);
  if (!rects) return { ok: false, reason: "selector not found" };
  const base = { button: "left", buttons: 1, clickCount: 1 };
  await send("Input.dispatchMouseEvent", { type: "mousePressed", ...base, x: rects.x1, y: rects.y1 });
  const steps = 6;
  for (let i = 1; i <= steps; i++) {
    await send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      button: "left",
      buttons: 1,
      x: rects.x1 + ((rects.x2 - rects.x1) * i) / steps,
      y: rects.y1 + ((rects.y2 - rects.y1) * i) / steps,
    });
  }
  await send("Input.dispatchMouseEvent", {
    type: "mouseReleased",
    button: "left",
    buttons: 0,
    clickCount: 1,
    x: rects.x2,
    y: rects.y2,
  });
  const end = await evalJs(`({
    cell: document.elementFromPoint(${rects.x2}, ${rects.y2})?.closest?.('td')?.dataset.cell || null,
    focus: window.__tt?.grid?.focus,
    info: window.__tt?.grid?.selectionInfo?.()
  })`);
  return { ok: true, end };
}

let result;
switch (cmd) {
  case "shot":
    result = { ok: true, file: await shot(args[0]) };
    break;
  case "eval":
    result = await evalJs(args.join(" "));
    break;
  case "click":
    result = await click(args[0]);
    break;
  case "clicktext":
    result = await clickText(args[0], args[1]);
    break;
  case "key":
    result = await key(args[0]);
    break;
  case "type":
    result = await typeInto(args[0], args.slice(1).join(" "));
    break;
  case "snap":
    result = await snap();
    break;
  case "theme":
    result = await setTheme(args[0]);
    break;
  case "drag":
    result = await drag(args[0], args[1]);
    break;
  default:
    console.error(`未知命令: ${cmd}`);
    process.exit(2);
}

console.log(JSON.stringify(result, null, 2));
ws.close();
