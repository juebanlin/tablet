// Univer 编辑窗口：一个窗口 = 一个 Group 工作簿（sheet = 该组节点）。
// 可以同时开多个窗口并排查看 / 比对；回写只进主进程内存，主界面保存才落盘（@docs/18）。

import { createUniver, LocaleType, mergeLocales } from "@univerjs/presets";
import { UniverSheetsCorePreset } from "@univerjs/preset-sheets-core";
import UniverPresetSheetsCoreZhCN from "@univerjs/preset-sheets-core/locales/zh-CN";
import "@univerjs/preset-sheets-core/lib/index.css";
import * as XLSX from "xlsx";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

const { univerAPI } = createUniver({
  locale: LocaleType.ZH_CN,
  locales: {
    [LocaleType.ZH_CN]: mergeLocales(UniverPresetSheetsCoreZhCN),
  },
  presets: [
    UniverSheetsCorePreset({
      container: "univer-container",
      // 自带 sheet bar 会允许新增/删除/重命名 sheet；这里关掉，换成下方只读切换条。
      footer: false,
    }),
  ],
});

// 当前窗口唯一的工作簿（一个窗口一个组）。
let ctx = null; // { project, group }
let wb = null;
let unitId = null;
let baseline = 0;
let dirty = false;
let applied = false;
let dirtyTimer = 0;

// ── xlsx ↔ Univer ────────────────────────────────────────────
// 数据往返完全复用主程序的 xlsx 布局：Table 4 行表头 / Constant / Enum 行式。
// 表头、sheet 名被改坏时由 Rust 侧 import_xlsx_into_group 整体拒绝并返回原因。
function cellToUniver(cell) {
  const v = cell.v;
  if (v === undefined || v === null || v === "") return null;
  if (cell.t === "n") return { v, t: 2 };
  if (cell.t === "b") return { v: v ? 1 : 0, t: 3 };
  if (cell.t === "d") {
    return { v: v instanceof Date ? v.toISOString() : String(v), t: 1 };
  }
  return { v: String(v), t: 1 };
}

function sheetToUniver(ws, name, id) {
  const range = XLSX.utils.decode_range(ws["!ref"] || "A1");
  const cellData = {};
  for (let r = range.s.r; r <= range.e.r; r++) {
    for (let c = range.s.c; c <= range.e.c; c++) {
      const cell = ws[XLSX.utils.encode_cell({ r, c })];
      if (!cell) continue;
      const item = cellToUniver(cell);
      if (!item) continue;
      (cellData[r] ??= {})[c] = item;
    }
  }
  // 预留空白行列，方便直接在表格尾部补数据。
  return {
    id,
    name,
    rowCount: Math.max(range.e.r + 120, 1000),
    columnCount: Math.max(range.e.c + 12, 26),
    defaultRowHeight: 24,
    defaultColumnWidth: 88,
    cellData,
  };
}

function workbookToUniver(b64, id, title) {
  const book = XLSX.read(b64, {
    type: "base64",
    cellDates: false,
    cellNF: false,
    cellStyles: false,
  });
  const sheetOrder = [];
  const sheets = {};
  book.SheetNames.forEach((name, i) => {
    const sid = `s${i}`;
    sheetOrder.push(sid);
    sheets[sid] = sheetToUniver(book.Sheets[name], name, sid);
  });
  return { id, name: title, sheetOrder, sheets };
}

function univerCellToAoa(cell) {
  if (!cell) return "";
  if (cell.t === 3) return !!cell.v;
  const v = cell.v;
  return v === undefined || v === null ? "" : v;
}

function cellDataToRows(cellData) {
  let maxR = -1;
  let maxC = -1;
  for (const rk of Object.keys(cellData || {})) {
    const row = cellData[rk];
    for (const ck of Object.keys(row || {})) {
      const cell = row[ck];
      if (!cell) continue;
      if (cell.v === undefined || cell.v === null || cell.v === "") continue;
      maxR = Math.max(maxR, Number(rk));
      maxC = Math.max(maxC, Number(ck));
    }
  }
  if (maxR < 0) return [[""]];
  const rows = [];
  for (let r = 0; r <= maxR; r++) {
    const line = [];
    for (let c = 0; c <= maxC; c++) line.push(univerCellToAoa(cellData[r]?.[c]));
    rows.push(line);
  }
  return rows;
}

function workbookToXlsx() {
  const snap = wb.save();
  const book = XLSX.utils.book_new();
  for (const sid of snap.sheetOrder) {
    const sheet = snap.sheets[sid];
    XLSX.utils.book_append_sheet(
      book,
      XLSX.utils.aoa_to_sheet(cellDataToRows(sheet.cellData)),
      sheet.name,
    );
  }
  return XLSX.write(book, { type: "base64", bookType: "xlsx", compression: true });
}

/** 工作簿内容指纹：只统计非空单元格的值，用于判断「有没有真的改过」。 */
function workbookFingerprint() {
  const snap = wb.save();
  const parts = [];
  for (const sid of snap.sheetOrder) {
    const sheet = snap.sheets[sid];
    parts.push(sheet.name);
    const cellData = sheet.cellData || {};
    for (const rk of Object.keys(cellData)) {
      const row = cellData[rk] || {};
      for (const ck of Object.keys(row)) {
        const cell = row[ck];
        if (!cell || cell.v === undefined || cell.v === null || cell.v === "") continue;
        parts.push(`${rk},${ck}=${cell.v}`);
      }
    }
    parts.push("\u001e");
  }
  const text = parts.join("\u001f");
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  }
  return h >>> 0;
}

function refreshDirty() {
  if (!wb) return;
  const next = workbookFingerprint() !== baseline;
  if (next === dirty) return;
  dirty = next;
  renderChrome();
}

function scheduleDirtyCheck() {
  clearTimeout(dirtyTimer);
  dirtyTimer = setTimeout(refreshDirty, 200);
}

// ── UI ──────────────────────────────────────────────────────
let toastTimer = 0;
function toast(message, kind = "ok") {
  const el = $("toast");
  el.textContent = message;
  el.className = `toast ${kind === "error" ? "error" : ""}`;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), kind === "error" ? 5200 : 2600);
}

function titleText() {
  const base = ctx ? `Univer 编辑 — ${ctx.group}` : "Univer 编辑";
  return dirty ? `${base} *` : base;
}

function renderChrome() {
  const ready = !!wb;
  $("btn-apply").disabled = !ready;
  $("btn-reload").disabled = !ready;
  $("dirty-dot").hidden = !dirty;
  $("sheet-name").textContent = ctx ? `${ctx.project} / ${ctx.group}` : "Univer 编辑";
  // 「已回写」只在本窗口真的执行过回写后显示；刚打开只是「已加载」。
  if (!ctx) {
    $("hint").textContent = "";
  } else if (dirty) {
    $("hint").textContent = "有未回写的改动 · Ctrl+S 回写（只改内存）";
  } else if (applied) {
    $("hint").textContent = "已回写内存 · 主界面点「保存」后落盘";
  } else {
    $("hint").textContent = "已加载 · 修改后点「回写」，只改内存，主界面保存后落盘";
  }
  invoke("set_title", { title: titleText() }).catch(() => {});
}

function showEmpty(text) {
  $("empty").textContent = text;
  $("empty").hidden = false;
}

/** 自绘 sheet 标签：只负责切换，不给增删/重命名入口。 */
function renderSheetTabs() {
  const bar = $("sheet-tabs");
  bar.textContent = "";
  if (!wb) return;
  const active = wb.getActiveSheet()?.getSheetId();
  const snap = wb.save();
  for (const sid of snap.sheetOrder) {
    const sheet = snap.sheets[sid];
    const el = document.createElement("button");
    el.className = `sheet-tab${sid === active ? " active" : ""}`;
    el.textContent = sheet.name;
    el.title = sheet.name;
    el.addEventListener("click", () => {
      const target = wb.getSheetBySheetId(sid);
      if (!target) return;
      wb.setActiveSheet(target);
      renderSheetTabs();
    });
    bar.append(el);
  }
}

// ── 打开 / 回写 / 重新加载 ──────────────────────────────────
async function openGroup(project, group) {
  ctx = { project, group };
  const payload = await invoke("univer_load_group", { project, group });
  const id = `u${Date.now()}`;
  const data = workbookToUniver(payload.xlsx_b64, id, `${project} / ${group}`);
  wb = univerAPI.createWorkbook(data, { makeCurrent: true });
  unitId = wb.getId();
  baseline = workbookFingerprint();
  dirty = false;
  applied = false;
  $("empty").hidden = true;
  renderChrome();
  renderSheetTabs();
}

async function applyActive() {
  if (!wb) return;
  try {
    const res = await invoke("univer_apply_group", {
      project: ctx.project,
      group: ctx.group,
      data: workbookToXlsx(),
    });
    baseline = workbookFingerprint();
    dirty = false;
    applied = true;
    renderChrome();
    toast(`已回写内存：${res.group}（${res.nodes_patched} 个节点），主界面保存后落盘`);
  } catch (e) {
    toast(String(e), "error");
  }
}

async function reloadActive() {
  if (!wb) return;
  refreshDirty();
  if (dirty && !confirm("有未回写的改动，重新加载将丢弃。确定继续？")) return;
  try {
    const { project, group } = ctx;
    if (unitId) univerAPI.disposeUnit(unitId);
    wb = null;
    unitId = null;
    await openGroup(project, group);
    toast(`已重新加载 ${group}`);
  } catch (e) {
    toast(String(e), "error");
  }
}

async function closeWindow() {
  refreshDirty();
  if (dirty && !confirm("有未回写的改动，关闭将丢弃。确定关闭窗口？")) return;
  const win = window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.();
  if (win) await win.destroy();
  else window.close();
}

// ── 事件 ─────────────────────────────────────────────────────
univerAPI.addEvent(univerAPI.Event.CommandExecuted, ({ id }) => {
  if (!id || !wb) return;
  if (id.includes("active-sheet")) renderSheetTabs();
  if (id.includes(".mutation.")) scheduleDirtyCheck();
});

$("btn-apply").addEventListener("click", () => void applyActive());
$("btn-reload").addEventListener("click", () => void reloadActive());
$("btn-close").addEventListener("click", () => void closeWindow());

document.addEventListener("keydown", (e) => {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  if (e.key.toLowerCase() === "s") {
    e.preventDefault();
    void applyActive();
  }
});

try {
  const win = window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.();
  win?.onCloseRequested?.(async (event) => {
    refreshDirty();
    if (!dirty) return;
    event.preventDefault();
    if (confirm("有未回写的改动，关闭将丢弃。确定关闭窗口？")) {
      await win.destroy();
    }
  });
} catch {}

async function boot() {
  try {
    const initial = await invoke("univer_initial_context");
    if (!initial) {
      showEmpty("没有窗口上下文，请从主界面点「Univer 编辑」重新打开");
      renderChrome();
      return;
    }
    await openGroup(initial.project, initial.group);
  } catch (e) {
    showEmpty(`加载失败：${e}`);
    toast(String(e), "error");
  }
}

window.__univerTest = {
  univerAPI,
  applyActive,
  reloadActive,
  closeWindow,
  dirty: () => dirty,
  context: () => ctx,
  refreshDirty,
};

renderChrome();
boot();
