// Univer 编辑窗口回归：一个组一个窗口 / 多窗口并存 / 编辑回写内存 / 主界面只读 / 保存落盘。
//
// 前提：
//   1. 已构建 crates/app-tauri/ui/univer（cd crates/app-tauri/univer-editor && npm run build）
//   2. 用带 hero/HeroBase、building/Building 的 fixture 建好工作区，并带 CDP 启动 app：
//      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 tablet-tauri.exe --gui --workdir=<dir>
//   3. 工作区里 HeroBase 第一条记录是 id=1001，Building 第一条是 id=1。
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.CDP_PORT || 9222;
const WORKDIR = process.env.WORKDIR || "target/tmp-univer";
const heroFile = path.join(WORKDIR, "projects", "demo", "config", "hero", "HeroBase.tbl");
const buildingFile = path.join(WORKDIR, "projects", "demo", "config", "building", "Building.tbl");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function pages() {
  return (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()).filter(
    (t) => t.type === "page",
  );
}

async function connectPage(page) {
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
  const evalJs = async (expression) => {
    const msg = await send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (msg.result?.exceptionDetails) {
      throw new Error(msg.result.exceptionDetails.exception?.description || "JS 异常");
    }
    return msg.result?.result?.value;
  };
  return { ws, evalJs };
}

async function until(fn, label, timeout = 25000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const v = await fn();
    if (v) return v;
    await sleep(200);
  }
  throw new Error(`超时: ${label}`);
}

async function mainConn() {
  const page = (await pages()).find((t) => t.url === "http://tauri.localhost/");
  if (!page) throw new Error("找不到主界面 CDP 页面");
  return connectPage(page);
}

/** 按组名连到对应的 Univer 窗口（多窗口时 URL 相同）。 */
async function univerConn(group) {
  for (const page of (await pages()).filter((t) => t.url.includes("/univer/"))) {
    const conn = await connectPage(page);
    const ctx = await conn.evalJs(`window.__univerTest?.context?.()?.group`).catch(() => null);
    if (ctx === group) return conn;
    conn.ws.close();
  }
  return null;
}

const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
  if (!cond) console.log(`FAIL ${name}:`, JSON.stringify(detail));
}

const main = await mainConn();

// 1. 打开 hero 窗口
await main.evalJs(
  `(async () => {
    __tt.selectTree("table", "demo", "hero", "HeroBase");
    await new Promise((r) => setTimeout(r, 800));
    document.getElementById("btn-univer").click();
    return true;
  })()`,
);
const hero = await until(() => univerConn("hero"), "hero 窗口");
await until(() => hero.evalJs(`!!window.__univerTest?.context()`), "hero 上下文");
const heroState = await hero.evalJs(
  `(() => ({
    ctx: window.__univerTest.context(),
    dirty: window.__univerTest.dirty(),
    sheets: [...document.querySelectorAll(".sheet-tab")].map((e) => e.textContent),
  }))()`,
);
check("一个窗口一个组", heroState.ctx?.group === "hero", heroState);
check("初始未脏", heroState.dirty === false, heroState);
check("多个 sheet 有切换条", heroState.sheets.length === 2, heroState);

// 同一个组再次点开只聚焦已有窗口，不重复创建。
const univerCount = (await pages()).filter((t) => t.url.includes("/univer/")).length;
await main.evalJs(
  `window.__TAURI__.core.invoke("open_univer_editor", { project: "demo", group: "hero" })`,
);
await sleep(600);
const univerCount2 = (await pages()).filter((t) => t.url.includes("/univer/")).length;
check("重复打开只聚焦", univerCount === univerCount2 && univerCount2 === 1, {
  univerCount,
  univerCount2,
});

// 2. 主界面整块只读 + 后端拦截
const locked = await main.evalJs(
  `(async () => {
    const s = await window.__TAURI__.core.invoke("snapshot");
    let err = null;
    try { await window.__TAURI__.core.invoke("commit_cell", { row: 4, col: 1, value: "x" }); }
    catch (e) { err = String(e); }
    return {
      overlay: !document.getElementById("grid-lock-overlay").hidden,
      locked: __tt.grid.locked,
      univer: s.status.univer_editing,
      err,
    };
  })()`,
);
check("主界面整块置灰", locked.overlay === true && locked.locked === true, locked);
check("状态栏列出 Univer 组", (locked.univer || []).includes("hero"), locked);
check("后端拦截编辑", !!locked.err, locked);

// 3. 编辑 hero 并回写（只进内存）
const beforeDisk = fs.readFileSync(heroFile, "utf8");
await hero.evalJs(
  `(() => {
    window.__univerTest.univerAPI.getActiveWorkbook().getActiveSheet().getRange(4, 1).setValue("亚瑟-U");
    return true;
  })()`,
);
await until(() => hero.evalJs(`window.__univerTest.dirty() === true`), "hero 脏标记");
await hero.evalJs(`window.__univerTest.applyActive()`);
await until(() => hero.evalJs(`window.__univerTest.dirty() === false`), "hero 回写清脏");

const applied = await main.evalJs(
  `(async () => {
    const s = await window.__TAURI__.core.invoke("snapshot");
    return { dirty: s.status.dirty, row: s.grid?.rows?.[0]?.slice(0, 2) };
  })()`,
);
check("内存已更新", applied.dirty === true && applied.row?.[1] === "亚瑟-U", applied);
check("未落盘", fs.readFileSync(heroFile, "utf8") === beforeDisk, "file changed early");

// 4. 第二个组 = 第二个窗口
await main.evalJs(
  `(async () => {
    __tt.selectTree("table", "demo", "building", "Building");
    await new Promise((r) => setTimeout(r, 700));
    document.getElementById("btn-univer").click();
    return true;
  })()`,
);
const building = await until(() => univerConn("building"), "building 窗口");
const both = await main.evalJs(
  `(async () => {
    const s = await window.__TAURI__.core.invoke("snapshot");
    return s.status.univer_editing;
  })()`,
);
check("两个组各开一个窗口", (both || []).sort().join(",") === "building,hero", both);

await building.evalJs(
  `(() => {
    window.__univerTest.univerAPI.getActiveWorkbook().getActiveSheet().getRange(4, 1).setValue("主城-U");
    return true;
  })()`,
);
await until(() => building.evalJs(`window.__univerTest.dirty() === true`), "building 脏标记");
await building.evalJs(`window.__univerTest.applyActive()`);
await until(() => building.evalJs(`window.__univerTest.dirty() === false`), "building 回写清脏");

// 5. 关闭窗口 → 解锁
await hero
  .evalJs(
    `(() => { window.__TAURI__.webviewWindow.getCurrentWebviewWindow().destroy(); return true; })()`,
  )
  .catch(() => {});
await building
  .evalJs(
    `(() => { window.__TAURI__.webviewWindow.getCurrentWebviewWindow().destroy(); return true; })()`,
  )
  .catch(() => {});
await sleep(1500);
const unlocked = await main.evalJs(
  `(async () => {
    const s = await window.__TAURI__.core.invoke("snapshot");
    return { locked: __tt.grid.locked, univer: s.status.univer_editing };
  })()`,
);
check("关窗后解锁", unlocked.locked === false && (unlocked.univer || []).length === 0, unlocked);

// 6. 保存落盘
await main.evalJs(
  `(async () => {
    document.getElementById("btn-save").click();
    await new Promise((r) => setTimeout(r, 1500));
    return true;
  })()`,
);
check("保存后 HeroBase 落盘", fs.readFileSync(heroFile, "utf8").includes("亚瑟-U"), "not found");
check(
  "保存后 Building 落盘",
  fs.readFileSync(buildingFile, "utf8").includes("主城-U"),
  "not found",
);

const failed = results.filter((r) => !r.ok);
console.log(results.map((r) => `${r.ok ? "PASS" : "FAIL"}  ${r.name}`).join("\n"));
console.log(failed.length ? `\n${failed.length} 项失败` : "\n全部通过");
main.ws.close();
process.exit(failed.length ? 1 : 0);
