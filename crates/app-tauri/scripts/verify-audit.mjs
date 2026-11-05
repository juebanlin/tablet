// 功能完整性自验：列宽持久化 / 显示值与原值分离 / 保存失败提示 / 全局设置生效 / 导出预设。
// 前提：全新工作区（建议 full fixture，含 hero/Hero）、CDP 启动 app，WORKDIR=<工作区绝对路径>。
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.CDP_PORT || 9222;
const WORKDIR = process.env.WORKDIR || "";
const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const page = targets.find((t) => t.type === "page" && !t.url.includes("/univer/"));
if (!page) throw new Error("没有找到主界面页面 target");

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
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++seq;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
async function evalJs(expression) {
  const msg = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (msg.result?.exceptionDetails) {
    throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
  }
  return msg.result?.result?.value;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(expr, label, timeout = 12000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await evalJs(expr)) return;
    await sleep(150);
  }
  throw new Error(`超时: ${label}`);
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

await until(`!!(window.__tt && __tt.state.snapshot)`, "首份快照");
await evalJs(
  `(async () => {
    __tt.selectTree("table", "demo", "hero", "HeroBase");
    await new Promise((r) => setTimeout(r, 800));
    return true;
  })()`,
);

// 1. 显示值与原值分离：kind=@HeroType，显示 WARRIOR，存储仍是 id。
await evalJs(
  `(async () => {
    document.getElementById("show-ref-name").checked = true;
    document.getElementById("show-ref-name").dispatchEvent(new Event("change"));
    await new Promise((r) => setTimeout(r, 700));
    return true;
  })()`,
);
const split = await evalJs(
  `(() => ({ raw: __tt.grid.cellText(0, 2), display: __tt.grid.displayCellText(0, 2) }))()`,
);
check("显示值与原值分离", split.raw === "1" && split.display === "WARRIOR", split);
const formulaRo = await evalJs(
  `(() => { __tt.grid.selectCell(0, 2, false); return document.getElementById("formula-input").readOnly; })()`,
);
check("引用列公式栏只读", formulaRo === true, formulaRo);

// 2. 列宽持久化：手动关自动列宽 + 拖宽 → snapshot + user-data.json。
await evalJs(
  `(async () => {
    __tt.grid.setAutoWidth(false);
    __tt.grid.setColWidth(0, 150);
    await new Promise((r) => setTimeout(r, 900));
    return true;
  })()`,
);
const view = await evalJs(
  `(async () => {
    const s = await window.__TAURI__.core.invoke("snapshot");
    return { auto: s.grid.auto_width, width: s.grid.column_widths.id };
  })()`,
);
check("列宽写入 snapshot", view.auto === false && view.width === 150, view);
const userData = path.join(WORKDIR, ".data", "user-data.json");
check("列宽写入 user-data.json", fs.existsSync(userData) && fs.readFileSync(userData, "utf8").includes('"id": 150'), userData);

// 3. 保存失败必须报错（重复 id），且不能弹“已保存”。
await evalJs(
  `(async () => {
    await __tt.runOp("select_node", { project: "demo", kind: "table", group: "hero", name: "HeroBase" });
    await __tt.runOp("commit_cell", { row: 1, col: 0, value: "1001" });
    return true;
  })()`,
);
const saveErr = await evalJs(
  `(async () => {
    try { await window.__TAURI__.core.invoke("save_workspace"); return null; }
    catch (e) { return String(e); }
  })()`,
);
check("保存失败向上报错", !!saveErr, saveErr);

// 4. 全局设置 show_meta_id 生效。
const label = await evalJs(
  `(async () => {
    const gs = await window.__TAURI__.core.invoke("global_settings");
    gs.show_meta_id = true;
    const snap = await window.__TAURI__.core.invoke("save_global_settings", { settings: gs });
    __tt.applySnapshot(snap);
    return snap.tree.find((r) => r.kind === "project")?.label;
  })()`,
);
check("项目列表显示元数据 ID", label === "demo", label);

// 5. 失焦自动提交关闭后，blur 不写回。
const blur = await evalJs(
  `(async () => {
    const gs = await window.__TAURI__.core.invoke("global_settings");
    gs.auto_commit_on_blur = false;
    const snap = await window.__TAURI__.core.invoke("save_global_settings", { settings: gs });
    __tt.applySnapshot(snap);
    __tt.selectTree("table", "demo", "hero", "HeroBase");
    await new Promise((r) => setTimeout(r, 700));
    const before = __tt.grid.cellText(0, 1);
    __tt.grid.beginCellEdit(0, 1, "x");
    const input = document.querySelector("#grid-scroll input");
    input.focus();
    input.value = "should-not-commit";
    input.blur();
    await new Promise((r) => setTimeout(r, 400));
    return { before, after: __tt.grid.cellText(0, 1) };
  })()`,
);
check("失焦提交开关生效", blur.before === blur.after, blur);

// 6. 导出预设。
const presets = await evalJs(
  `(() => {
    document.getElementById("btn-export").click();
    const counts = {};
    for (const key of ["data", "server", "client", "all"]) {
      document.querySelector('#export-dialog [data-expre="' + key + '"]').click();
      counts[key] = [...document.querySelectorAll("#ex-formats input[data-fmt]")].filter((i) => i.checked).length;
    }
    document.getElementById("ex-cancel").click();
    return counts;
  })()`,
);
check(
  "导出预设 数据/后端/前端/全部",
  presets.data === 2 && presets.server === 7 && presets.client === 5 && presets.all === 12,
  presets,
);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
ws.close();
process.exit(failed.length ? 1 : 0);
