// Tauri 版主界面：Toolbar / TreeSection / GridSection / BottomPanel。
// 数据全部来自 Rust 侧 `snapshot`，前端只做渲染与交互（@docs/18 §1）。

const $ = (id) => document.getElementById(id);

const state = {
  snapshot: null,
  selected: null, // { project, kind, group, name }
  expandedProjects: new Set(),
  expandedGroups: new Set(), // `${project}|${group}`
  treeSearch: "",
  treeFilter: 0,
  treeSort: 0,
  fullGroup: false,
  logs: [],
  seenLogs: new Set(),
  logFilter: 0,
  bottomTab: 0,
  initialized: false,
  // 正在 Univer 窗口中打开的组（主界面只读，防止两边同时改）。
  univerEditing: new Set(),
  // 已提示过「外部 xlsx 修改但有本地脏改动」的组，避免每 2s 重复 toast。
  xlsxWarned: new Set(),
  // 启动时打不开的项目只提示一次。
  startupAlertsShown: false,
  repairingSelection: false,
};

let grid;
let opChain = Promise.resolve();

// ── 与 Rust 通信 ────────────────────────────────────────────
async function invoke(cmd, args) {
  const api = window.__TAURI__?.core?.invoke;
  if (!api) throw new Error("未连接 Rust 侧（浏览器预览）");
  return api(cmd, args);
}

/** 串行执行后端操作，避免连续编辑时快照乱序。
 * 注意：失败必须让本次调用 reject（调用方不再弹“成功”toast），但队列不能断，
 * 所以 opChain 吞掉错误，只有返回给调用方的 promise 带失败。 */
function runOp(cmd, args) {
  const op = opChain
    .then(() => invoke(cmd, args))
    .then((snap) => applySnapshot(snap));
  opChain = op.catch(() => {});
  op.catch((e) => {
    addLocalLog(2, String(e));
    toast(String(e), "error");
  });
  return op;
}

/** 单元格提交的轻量路径：不回整份 Snapshot，只更新状态 / 树 / 错误标记。 */
function fastCommitCell(row, col, value) {
  const op = opChain.then(() =>
    invoke("commit_cell_fast", { row, col, value }),
  );
  opChain = op.catch(() => {});
  op.then((res) => applyEditResult(res)).catch((e) => {
    addLocalLog(2, String(e));
    toast(String(e), "error");
    // 后端拒绝时本地单元格已被乐观改掉，拉一次完整快照恢复。
    invoke("snapshot").then(applySnapshot).catch(() => {});
  });
  return op;
}

/** 用轻量编辑结果更新 chrome；不碰 grid.rows（本地已经改过）。 */
function applyEditResult(res) {
  const s = state.snapshot;
  if (!s || !res) return;
  s.status = res.status;
  s.tree = res.tree;
  s.issues = res.issues;
  const st = res.status;
  $("dirty-dot").hidden = !st.dirty;
  $("btn-save").disabled = !st.dirty;
  $("btn-undo").disabled = !st.can_undo;
  $("btn-redo").disabled = !st.can_redo;
  $("btn-undo").title = st.undo_label || "";
  $("btn-redo").title = st.redo_label || "";
  $("btn-revert").disabled = !st.can_revert || grid.locked;
  renderTree();
  renderIssues();
  grid.setErrors(res.errors || []);
  onGridSelection(currentSelection());
}

// ── 日志 ────────────────────────────────────────────────────
function nowStamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function addLocalLog(level, msg) {
  state.logs.unshift({ time: nowStamp(), level, message: msg });
  renderLog();
}

function mergeBackendLogs(rows) {
  let added = false;
  for (const r of rows) {
    const key = `${r.time}|${r.message}`;
    if (state.seenLogs.has(key)) continue;
    state.seenLogs.add(key);
    const level = r.level === "ERROR" ? 2 : r.level === "WARN" ? 1 : 0;
    state.logs.push({ time: r.time, level, message: r.message });
    added = true;
  }
  if (added) {
    state.logs.sort((a, b) => (a.time < b.time ? 1 : a.time > b.time ? -1 : 0));
    if (state.logs.length > 1000) state.logs.length = 1000;
    renderLog();
  }
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

function renderLog() {
  const lvName = ["INFO", "WARN", "ERROR"];
  const filtered = state.logs.filter((l) => state.logFilter === 0 || l.level === state.logFilter - 1);
  $("log-list").innerHTML = filtered
    .map(
      (l) =>
        `<div class="log-line ${["", "warn", "error"][l.level]}">` +
        `<span class="ts">${escapeHtml(l.time)}</span>` +
        `<span class="lv">${lvName[l.level]}</span>` +
        `<span class="msg">${escapeHtml(l.message)}</span></div>`,
    )
    .join("");
  $("log-summary").textContent = `${filtered.length} 条`;
}

// ── 快照 ────────────────────────────────────────────────────
function applySnapshot(s) {
  if (!s) return;
  state.snapshot = s;
  if (!state.startupAlertsShown && (s.startup_alerts || []).length) {
    state.startupAlertsShown = true;
    openModal({
      title: "部分项目打不开",
      message: s.startup_alerts.join("\n\n"),
      input: false,
    });
  }
  // 撤销 / 删除 / 重新加载后，选中的节点可能已不存在：退回项目根。
  if (state.selected?.name) {
    const sel = state.selected;
    const exists = (s.tree || []).some(
      (r) =>
        r.kind === sel.kind &&
        r.project === sel.project &&
        r.group === sel.group &&
        r.name === sel.name,
    );
    if (!exists) {
      const project = sel.project || s.status.active_project;
      state.selected = project
        ? { project, kind: "project", group: null, name: null }
        : null;
      if (project && !state.repairingSelection) {
        state.repairingSelection = true;
        invoke("select_node", { project, kind: "project", group: null, name: null })
          .then((snap) => {
            state.repairingSelection = false;
            applySnapshot(snap);
          })
          .catch(() => {
            state.repairingSelection = false;
          });
      }
    }
  }

  if (!state.initialized) {
    state.initialized = true;
    // 默认展开当前项目及其所有组（与 Slint 版启动观感一致）。
    for (const row of s.tree) {
      if (row.kind === "project" && row.active && !row.closed) {
        state.expandedProjects.add(row.project);
      }
      if (row.kind === "group" && row.active) {
        state.expandedGroups.add(`${row.project}|${row.group}`);
      }
    }
    if (s.status.active_project) state.expandedProjects.add(s.status.active_project);
    for (const row of s.tree) {
      if (row.kind === "group" && row.project === s.status.active_project) {
        state.expandedGroups.add(`${row.project}|${row.group}`);
      }
    }
  }

  const st = s.status;
  const sortIdx = ["id", "name", "open", "created", "manual"].indexOf(st.project_sort || "id");
  if (sortIdx >= 0) {
    state.treeSort = sortIdx;
    if (document.activeElement !== $("sort-select")) $("sort-select").selectedIndex = sortIdx;
  }
  $("project-label").textContent = `当前项目：${st.active_project_name || "—"}`;
  if (document.activeElement !== $("tree-show-id")) {
    $("tree-show-id").checked = !!st.show_meta_id;
  }
  $("dirty-dot").hidden = !st.dirty;
  $("btn-save").disabled = !st.dirty;
  $("btn-undo").disabled = !st.can_undo;
  $("btn-redo").disabled = !st.can_redo;  $("btn-undo").title = st.undo_label || "";
  $("btn-redo").title = st.redo_label || "";
  $("excel-session").textContent = (st.excel_editing || []).length
    ? `Excel 编辑中：${st.excel_editing.join("、")}`
    : "";
  state.univerEditing = new Set(st.univer_editing || []);
  const selectedGroup = state.selected?.group;
  const sameProject = state.selected?.project === st.active_project;
  const lockedByUniver = !!selectedGroup && sameProject && state.univerEditing.has(selectedGroup);
  const lockedByExcel =
    !!selectedGroup &&
    sameProject &&
    (st.excel_editing || []).includes(selectedGroup);
  const lockedGroup = lockedByUniver || lockedByExcel ? selectedGroup : null;
  $("univer-session").textContent = (st.univer_editing || []).length
    ? `Univer 编辑中（主界面只读）：${st.univer_editing.join("、")}`
    : "";
  grid.setLocked(!!lockedGroup);
  grid.setAutoCommitOnBlur(st.auto_commit_on_blur !== false);
  $("grid-lock-overlay").hidden = !lockedGroup;
  if (lockedGroup) {
    document.querySelector(".grid-lock-title").textContent = lockedByExcel
      ? "该组正在 Excel 中编辑"
      : "该组正在 Univer 窗口中编辑";
    document.querySelector(".grid-lock-sub").textContent = lockedByExcel
      ? "Excel 改动会自动回写内存；主界面此时只读"
      : "主界面此时只读，避免两边同时修改数据";
    $("grid-lock-open").textContent = lockedByExcel ? "结束 Excel 编辑" : "聚焦 Univer 窗口";
  }
  if (lockedGroup || (st.univer_editing || []).length || (st.excel_editing || []).length) {
    // 撤销/重做是项目级的，可能改到任何被外部编辑器占用的组；锁定期先禁用。
    $("btn-undo").disabled = true;
    $("btn-redo").disabled = true;
  }

  document.title = `tablet v${s.version} — ${s.vcs}`;
  invoke("set_title", { title: `tablet v${s.version} — ${s.vcs}` }).catch(() => {});

  renderTree();
  mergeBackendLogs(s.logs);
  renderIssues();

  const g = s.grid;
  $("grid-title").textContent = g ? g.title : "未选中";
  $("grid-subtitle").textContent = g ? g.subtitle : "";
  $("grid-empty").hidden = !!g;
  $("grid-scroll").hidden = !g;
  $("btn-rules").disabled = !g || g.mode === "constant" || !!lockedGroup;
  $("btn-excel").disabled = !g || !!lockedGroup;
  $("btn-univer").disabled = !g;
  $("btn-univer").title = lockedGroup
    ? "该组已在 Univer 窗口中打开（点击聚焦窗口）"
    : "用内置 Univer 打开当前组（一个组一个窗口，可多开）";
  $("btn-revert").disabled = !st.can_revert || !!lockedGroup;
  $("show-ref-name").disabled = !g || g.mode === "enum" || !!lockedGroup;
  $("display-field").disabled = !g || !!lockedGroup || !(g.display_options || []).length;

  const showRef = $("show-ref-name");
  if (document.activeElement !== showRef) showRef.checked = !!st.show_ref_name;
  $("auto-width").checked = g ? g.auto_width !== false : true;
  const df = $("display-field");
  const options = g?.display_options || [];
  const current = g?.display_field || "";
  df.innerHTML =
    '<option value="">ID</option>' +
    options
      .map(
        (c) =>
          `<option value="${escapeHtml(c)}"${c === current ? " selected" : ""}>${escapeHtml(c)}</option>`,
      )
      .join("");
  df.value = current;
  df.disabled = !g || !!lockedGroup || options.length === 0;
  if (!g) {
    grid.setData(null);
    $("formula-bar").hidden = true;
  } else {
    grid.setData(toGridNode(g), { keepSelection: true });
  }
}

function toGridNode(g) {
  return {
    mode: g.mode,
    title: g.title,
    subtitle: g.subtitle,
    headers: g.headers,
    headerKinds: g.header_kinds,
    kinds: g.kinds,
    rows: g.rows,
    displayRows: g.display_rows || [],
    errors: g.errors,
    dataCount: g.data_count,
    editableHeaders: g.editable_headers,
    refTargets: g.ref_targets || [],
    refCols: g.ref_cols || [],
    refList: g.ref_list || [],
    pickerHeaderSingle: g.picker_header_single !== false,
    pickerDataSingle: !!g.picker_data_single,
    autoWidth: g.auto_width !== false,
    columnWidths: g.column_widths || {},
  };
}

// 列宽 / 自动列宽变化 → 防抖写入 `.data/user-data.json`（不进项目 dirty）。
let gridViewTimer = 0;
function saveGridView(view) {
  const sel = state.selected;
  if (!sel?.project || !sel.group || !sel.name) return;
  // 手动拖列宽会自动关闭自动列宽，勾选框同步。
  $("auto-width").checked = view.auto !== false;
  const target = { ...sel };
  clearTimeout(gridViewTimer);
  gridViewTimer = setTimeout(() => {
    invoke("save_grid_view", {
      project: target.project,
      group: target.group,
      kind: target.kind,
      name: target.name,
      auto: view.auto,
      widths: view.widths,
    }).catch((e) => addLocalLog(2, String(e)));
  }, 400);
}

// ── 树 ──────────────────────────────────────────────────────
function treeModel(rows) {
  const projects = [];
  let project = null;
  let group = null;
  for (const row of rows) {
    if (row.kind === "project") {
      project = { ...row, groups: [] };
      projects.push(project);
      group = null;
    } else if (row.kind === "group" && project) {
      group = { ...row, nodes: [] };
      project.groups.push(group);
    } else if (project && group) {
      group.nodes.push(row);
    }
  }
  return projects;
}

function nodeIcon(kind) {
  return { table: "📊", constant: "📋", enum: "🔢" }[kind] || "📄";
}

function filterPass(row) {
  if (state.treeFilter === 0) return true;
  if (state.treeFilter === 1) return !!row.mark; // 改动
  const want = ["", "", "+", "*", "-"][state.treeFilter];
  return row.mark === want;
}

/** 后端给的 search_key = 小写名字 + 拼音首字母；直接 contains 即可。 */
function searchPass(key) {
  if (!state.treeSearch) return true;
  return (key || "").includes(state.treeSearch.toLowerCase());
}

function isSelected(kind, project, group, name) {
  const s = state.selected;
  if (!s || s.kind !== kind || s.project !== project) return false;
  if (kind === "project") return true;
  if (kind === "group") return s.group === group;
  return s.group === group && s.name === name;
}

function treeRowHtml({ key, indent, caret, icon, label, mark, markColor, selected, closed, extra }) {
  const caretHtml =
    caret == null
      ? '<span class="tree-caret placeholder">▾</span>'
      : `<span class="tree-caret" data-toggle="${key}">${caret}</span>`;
  return (
    `<div class="tree-item${selected ? " selected" : ""}${closed ? " closed" : ""}" ` +
    `data-key="${key}" style="padding-left:${4 + indent * 14}px">` +
    caretHtml +
    `<span class="tree-icon">${icon}</span>` +
    `<span class="tree-name">${escapeHtml(label)}${extra || ""}</span>` +
    (mark ? `<span class="tree-mark" style="color:${markColor}">${mark}</span>` : "") +
    `</div>`
  );
}

function renderTree() {
  const s = state.snapshot;
  if (!s) return;
  const projects = treeModel(s.tree);
  const byName = (a, b) => a.label.localeCompare(b.label, "zh");
  if (state.treeSort === 0) projects.sort((a, b) => a.project.localeCompare(b.project));
  else if (state.treeSort === 1) projects.sort(byName);
  else if (state.treeSort === 2) {
    projects.sort((a, b) => Number(b.active) - Number(a.active) || byName(a, b));
  } else if (state.treeSort === 3) projects.sort(byName); // 无创建时间字段，退回名称
  // 4 = 手动：保持后端顺序
  const out = [];

  for (const p of projects) {
    const openExpanded = !p.closed && state.expandedProjects.has(p.project);

    // 先算组/节点命中，再决定项目根是否展示（与 Slint 的“任一子项命中即展示”一致）。
    const groupViews = [];
    if (!p.closed) {
      for (const g of p.groups) {
        const visibleNodes = state.fullGroup
          ? g.nodes.filter((n) => n.mark !== "-")
          : g.nodes.filter((n) => searchPass(n.search_key) && filterPass(n));
        const groupHit = searchPass(g.search_key);
        if (state.fullGroup || groupHit || visibleNodes.length > 0) {
          groupViews.push({ g, visibleNodes });
        }
      }
    }
    if (state.treeSearch && !p.closed && !searchPass(p.search_key) && groupViews.length === 0) {
      continue;
    }

    const pKey = `p:${p.project}`;
    const pSelected = isSelected("project", p.project, null, null);
    const caret = p.closed ? null : openExpanded ? "▼" : "▶";
    out.push(
      treeRowHtml({
        key: pKey,
        indent: 0,
        caret,
        icon: "📦",
        label: p.label,
        mark: p.mark,
        markColor: p.mark_color,
        selected: pSelected,
        closed: p.closed,
        extra: p.active ? " ★" : "",
      }),
    );
    if (!openExpanded) continue;

    for (const { g, visibleNodes } of groupViews) {
      const gKey = `g:${p.project}|${g.group}`;
      const gExpanded = state.expandedGroups.has(`${p.project}|${g.group}`);

      out.push(
        treeRowHtml({
          key: gKey,
          indent: 1,
          caret: gExpanded ? "▼" : "▶",
          // 与 Slint 一致：xlsx 组（节点 = 工作簿 sheet）用 📗，tbl 组用目录 📁。
          icon: g.xlsx ? "📗" : "📁",
          label: g.label,
          mark: g.mark,
          markColor: g.mark_color,
          selected: isSelected("group", p.project, g.group, null),
          closed: false,
          extra: groupLocked(p.project, g.group) ? " 🔒" : "",
        }),
      );
      if (!gExpanded) continue;

      for (const n of visibleNodes) {
        out.push(
          treeRowHtml({
            key: `n:${n.kind}:${p.project}|${g.group}|${n.name}`,
            indent: 2,
            caret: null,
            icon: nodeIcon(n.kind),
            label: n.label,
            mark: n.mark,
            markColor: n.mark_color,
            selected: isSelected(n.kind, p.project, g.group, n.name),
            closed: false,
          }),
        );
      }
    }
  }

  $("tree-list").innerHTML = out.join("");
}

function onTreeClick(e) {
  const toggle = e.target.closest("[data-toggle]");
  if (toggle) {
    const key = toggle.dataset.toggle;
    if (key.startsWith("p:")) {
      const id = key.slice(2);
      if (state.expandedProjects.has(id)) state.expandedProjects.delete(id);
      else state.expandedProjects.add(id);
    } else if (key.startsWith("g:")) {
      const k = key.slice(2);
      if (state.expandedGroups.has(k)) state.expandedGroups.delete(k);
      else state.expandedGroups.add(k);
    }
    renderTree();
    return;
  }

  const item = e.target.closest(".tree-item");
  if (!item) return;
  const key = item.dataset.key;
  if (key.startsWith("p:")) {
    const id = key.slice(2);
    state.expandedProjects.add(id);
    selectTree("project", id, null, null);
  } else if (key.startsWith("g:")) {
    const [project, group] = key.slice(2).split("|");
    state.expandedGroups.add(`${project}|${group}`);
    selectTree("group", project, group, null);
  } else if (key.startsWith("n:")) {
    const t = decodeTreeKey(key);
    selectTree(t.kind, t.project, t.group, t.name);
  }
}

function decodeTreeKey(key) {
  if (key.startsWith("p:")) return { kind: "project", project: key.slice(2) };
  if (key.startsWith("g:")) {
    const [project, group] = key.slice(2).split("|");
    return { kind: "group", project, group };
  }
  // n:<kind>:<project>|<group>|<name> —— kind 与 project 之间是冒号，
  // 与 project/group/name 之间的竖线不是同一个分隔符。
  const rest = key.slice(2);
  const colon = rest.indexOf(":");
  const kind = rest.slice(0, colon);
  const [project, group, name] = rest.slice(colon + 1).split("|");
  return { kind, project, group, name };
}

function selectTree(kind, project, group, name) {
  state.selected = { project, kind, group, name };
  if (kind !== "project") state.expandedProjects.add(project);
  if (group) state.expandedGroups.add(`${project}|${group}`);
  renderTree();
  runOp("select_node", { project, kind, group, name });
}

// ── 公式栏 / 状态栏 ─────────────────────────────────────────
function onGridSelection(sel) {
  $("selection-info").textContent = sel.info;
  $("hover-info").textContent = sel.hover ? `光标: ${sel.hover}` : "";
  // 单格有校验问题时，状态栏在坐标后追加原因（与 Slint 版一致：即时重算）。
  if (sel.kind === "cell") {
    const r = grid.focus.r;
    const c = grid.focus.c;
    invoke("cell_issue", { row: r, col: c })
      .then((issue) => {
        if (grid.kind !== "cell" || grid.focus.r !== r || grid.focus.c !== c) return;
        let text = grid.selectionInfo();
        if (issue) {
          text += issue.severity === "error" ? ` ⚠ ${issue.message}` : ` 警告：${issue.message}`;
        }
        $("selection-info").textContent = text;
      })
      .catch(() => {});
  }
  const input = $("formula-input");
  const bar = $("formula-bar");
  // 只要有选中节点就保留公式栏高度：避免“点第一下格子时公式栏突然出现，
  // 整个表格向下跳一行”——拖动选区会在按下瞬间被这 28px 位移带偏。
  bar.hidden = !grid.node;
  if (!grid.node) return;
  // 展开态只在单格选区有效；切到区域 / 整行 / 整列时收起（与 Slint 版一致）。
  if (sel.kind !== "cell" && bar.classList.contains("expanded")) {
    if (!input.readOnly) grid.setActiveValue(input.value);
    bar.classList.remove("expanded");
  }
  $("formula-coord").textContent = sel.coord || "—";
  if (document.activeElement !== input) input.value = sel.value;
  input.readOnly = !sel.editable;
  input.placeholder = sel.editable ? "" : sel.kind === "none" ? "选择单元格" : "只读";
  $("formula-lock").hidden = !!sel.editable || sel.kind === "none";
}

function initFormulaBar() {
  const input = $("formula-input");
  const expanded = () => $("formula-bar").classList.contains("expanded");
  input.addEventListener("keydown", (e) => {
    // 展开态是多行编辑：Enter 换行；折叠态 Enter 提交并下移。
    if (e.key === "Enter" && !expanded()) {
      e.preventDefault();
      grid.setActiveValue(input.value);
      grid.selectCell(grid.focus.r + 1, grid.focus.c, false);
      input.blur();
    } else if (e.key === "Escape") {
      onGridSelection(currentSelection());
      input.blur();
    }
    e.stopPropagation();
  });
  input.addEventListener("blur", () => {
    if (grid.node && !input.readOnly) grid.setActiveValue(input.value);
  });
  $("formula-toggle").addEventListener("click", () => {
    const bar = $("formula-bar");
    const wasExpanded = bar.classList.contains("expanded");
    bar.classList.toggle("expanded");
    if (wasExpanded && grid.node && !input.readOnly) {
      // 收起 = 提交多行缓冲
      grid.setActiveValue(input.value);
    }
  });
}

function currentSelection() {
  const c = (i) => colLetter(i);
  return {
    kind: grid.kind,
    info: grid.selectionInfo(),
    coord: grid.kind === "none" ? "" : `${c(grid.focus.c)}${grid.focus.r + 1}`,
    value: grid.kind === "cell" ? grid.cellText(grid.focus.r, grid.focus.c) : "",
    editable:
      grid.kind === "cell" && !grid.isReadonlyCol(grid.focus.c) && !grid.isPickerCol(grid.focus.c),
    hover: grid.hover ? `${c(grid.hover.c)}${grid.hover.r + 1}` : "",
  };
}

// ── 工具栏 ──────────────────────────────────────────────────
function initToolbar() {
  $("btn-undo").addEventListener("click", () => runOp("undo", {}));
  $("btn-redo").addEventListener("click", () => runOp("redo", {}));
  $("btn-save").addEventListener("click", () => {
    addLocalLog(0, "保存工作区…");
    runOp("save_workspace", {}).then(() => toast("工作区已保存", "ok"));
  });
  $("btn-reload").addEventListener("click", async () => {
    // 有未保存改动先确认（与 Slint 版一致），不能直接丢掉内存里的修改。
    if (state.snapshot?.status?.dirty) {
      const ok = await openModal({
        title: "重新加载",
        message: "工作区有未保存的改动，重新加载会丢弃这些改动。确定继续？",
        input: false,
      });
      if (!ok) return;
    }
    addLocalLog(0, "重新加载工作区…");
    runOp("reload_workspace", {});
  });
  $("btn-validate").addEventListener("click", () => {
    runOp("validate_workspace", {}).then(() => {
      const st = state.snapshot?.status;
      const errs = st?.error_count ?? 0;
      const warns = st?.warning_count ?? 0;
      toast(`校验完成：${errs} 错误 / ${warns} 警告`, errs ? "error" : "ok");
    });
  });
  $("btn-rules").addEventListener("click", () => openRulesEditor());
  $("btn-univer").addEventListener("click", () =>
    univerEdit(state.selected?.project, state.selected?.group),
  );
  $("grid-lock-open").addEventListener("click", () => {
    const sel = state.selected;
    if (!sel?.group) return;
    if (state.univerEditing.has(sel.group)) univerEdit(sel.project, sel.group);
    else runOp("excel_end_edit", { project: sel.project, group: sel.group });
  });
  $("btn-new-project").addEventListener("click", () => openCreateProject());
  $("btn-revert").addEventListener("click", () => {
    const sel = state.selected;
    if (!sel || !sel.name) return;
    runOp("revert_node", { project: sel.project, group: sel.group, name: sel.name });
  });
  $("auto-width").addEventListener("change", (e) => grid.setAutoWidth(e.target.checked));
  $("show-ref-name").addEventListener("change", (e) =>
    runOp("set_show_ref_name", { on: e.target.checked }),
  );
  $("display-field").addEventListener("change", (e) =>
    runOp("set_display_field", { field: e.target.value || null }),
  );

  document.addEventListener("keydown", (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    // 设置 / 对话框里的 Ctrl+Z 应作用于输入框，不要触发工作区撤销。
    const tag = e.target?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || e.target?.isContentEditable) {
      return;
    }
    const k = e.key.toLowerCase();
    if (k === "s") {
      e.preventDefault();
      $("btn-save").click();
    } else if (k === "z" || k === "y") {
      e.preventDefault();
      const anyLocked =
        (status().univer_editing || []).length || (status().excel_editing || []).length;
      if (anyLocked) {
        return toast("有组正在 Excel / Univer 中编辑，暂不能撤销/重做", "error");
      }
      runOp(k === "y" || (k === "z" && e.shiftKey) ? "redo" : "undo", {});
    }
  });
}

// ── 底部面板 ────────────────────────────────────────────────
function initBottomPanel() {
  $("bottom-tabs").addEventListener("click", (e) => {
    const tab = e.target.closest(".tab");
    if (!tab) return;
    state.bottomTab = +tab.dataset.tab;
    document.querySelectorAll("#bottom-tabs .tab").forEach((t) => t.classList.toggle("active", t === tab));
    document.querySelectorAll(".bottom-body .pane").forEach((p) => {
      p.hidden = +p.dataset.pane !== state.bottomTab;
    });
    if (state.bottomTab === 1) refreshExcelFiles();
    if (state.bottomTab === 2) renderIssues();
  });
  $("btn-clear-log").addEventListener("click", () => {
    state.logs = [];
    state.seenLogs.clear();
    renderLog();
    runOp("clear_logs", {}).catch(() => {});
  });
  document.querySelectorAll(".chip[data-filter]").forEach((chip) => {
    chip.addEventListener("click", () => {
      state.logFilter = +chip.dataset.filter;
      document.querySelectorAll(".chip[data-filter]").forEach((c) => c.classList.toggle("active", c === chip));
      renderLog();
    });
  });
}

// ── 分隔线 ──────────────────────────────────────────────────
function initSplitters() {
  const tree = $("tree-pane");
  const bottom = $("bottom-panel");
  $("splitter-v").addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = tree.getBoundingClientRect().width;
    const move = (ev) => (tree.style.width = Math.max(220, Math.min(startW + ev.clientX - startX, 800)) + "px");
    const up = () => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", up);
  });
  $("splitter-h").addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = bottom.getBoundingClientRect().height;
    const move = (ev) =>
      (bottom.style.height = Math.max(40, Math.min(startH - (ev.clientY - startY), 600)) + "px");
    const up = () => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", up);
  });
}

// ── 右键菜单 / 弹窗 ─────────────────────────────────────────
function hideContextMenu() {
  $("ctx-menu").hidden = true;
}

function showContextMenu(x, y, items) {
  const menu = $("ctx-menu");
  menu.innerHTML = "";
  // 与 Slint 版一致：不可用的项直接不出现（不再灰显）；顺带去掉空分隔线。
  const list = [];
  for (const it of items) {
    if (it.sep) {
      if (list.length && !list[list.length - 1].sep) list.push(it);
      continue;
    }
    if (it.disabled) continue;
    list.push(it);
  }
  while (list.length && list[list.length - 1].sep) list.pop();
  if (!list.length) {
    menu.hidden = true;
    return;
  }
  for (const it of list) {
    if (it.sep) {
      const d = document.createElement("div");
      d.className = "ctx-sep";
      menu.appendChild(d);
      continue;
    }
    const d = document.createElement("div");
    d.className = "ctx-item" + (it.danger ? " danger" : "");
    d.textContent = it.label;
    d.addEventListener("click", () => {
      hideContextMenu();
      Promise.resolve(it.action()).catch((e) => addLocalLog(2, String(e)));
    });
    menu.appendChild(d);
  }
  menu.hidden = false;
  const r = menu.getBoundingClientRect();
  menu.style.left = Math.max(0, Math.min(x, window.innerWidth - r.width - 4)) + "px";
  menu.style.top = Math.max(0, Math.min(y, window.innerHeight - r.height - 4)) + "px";
}

let modalResolve = null;

let modalValidator = null;
let modalValidateSeq = 0;

/** 输入校验结果写回弹窗：有错误时禁用「确定」（与 Slint 的实时校验一致）。 */
async function runModalValidation() {
  if (!modalValidator) return;
  const seq = ++modalValidateSeq;
  const value = $("modal-input").value.trim();
  let err = null;
  try {
    err = await modalValidator(value);
  } catch (e) {
    err = String(e);
  }
  if (seq !== modalValidateSeq) return;
  $("modal-error").textContent = err || "";
  $("modal-error").hidden = !err;
  $("modal-ok").disabled = !!err;
}

function openModal({ title, message, input = true, value = "", placeholder = "", xlsx = false, validate = null }) {
  return new Promise((resolve) => {
    modalResolve = resolve;
    modalValidator = validate;
    $("modal-error").hidden = true;
    $("modal-error").textContent = "";
    $("modal-ok").disabled = false;
    $("modal-input").value = value;
    if (validate) runModalValidation();
    $("modal-title").textContent = title;
    $("modal-message").textContent = message || "";
    $("modal-message").hidden = !message;
    const field = $("modal-input");
    field.hidden = !input;
    field.value = value;
    field.placeholder = placeholder;
    $("modal-xlsx-row").hidden = !xlsx;
    $("modal-xlsx").checked = false;
    $("modal").hidden = false;
    if (input) {
      field.focus();
      field.select();
    } else {
      $("modal-ok").focus();
    }
  });
}

function closeModal(result) {
  $("modal").hidden = true;
  modalValidator = null;
  modalValidateSeq++;
  $("modal-ok").disabled = false;
  const r = modalResolve;
  modalResolve = null;
  if (r) r(result);
}

function initModals() {
  $("modal-ok").addEventListener("click", () =>
    closeModal({ value: $("modal-input").value.trim(), xlsx: $("modal-xlsx").checked }),
  );
  $("modal-cancel").addEventListener("click", () => closeModal(null));
  $("modal-input").addEventListener("input", () => {
    if (modalValidator) runModalValidation();
  });
  $("modal-input").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      $("modal-ok").click();
    } else if (e.key === "Escape") {
      $("modal-cancel").click();
    }
    e.stopPropagation();
  });
  // 确认型弹窗（input:false，焦点不在输入框）也要能用 Esc 取消。
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("modal").hidden) {
      e.preventDefault();
      $("modal-cancel").click();
    }
  });
}

function status() {
  return state.snapshot?.status || {};
}

/** 该组是否正在 Univer 窗口中编辑（主界面只读，防止数据打架）。 */
function groupLocked(project, group) {
  const st = status();
  return project === st.active_project && (st.univer_editing || []).includes(group);
}

async function pickPath(title, ext) {
  return invoke("pick_file", { title, extensions: [ext] }).catch(() => null);
}

async function pickPaths(title, ext) {
  return invoke("pick_files", { title, extensions: [ext] }).catch(() => null);
}

async function pasteNodeFmt(project, group, fmt) {
  await runOp("paste_node", { project, group, fmt: fmt || null });
}

// 粘贴 Group：无冲突直接粘；有重名时弹命名确认（与 Slint 版一致）。
let pgState = null;

async function pasteGroupFmt(project, storage) {
  let preview;
  try {
    preview = await invoke("paste_group_preview", { project });
  } catch (e) {
    toast(String(e), "error");
    return;
  }
  if (!preview) {
    addLocalLog(1, "剪贴板里没有 Group");
    return;
  }
  if (!preview.has_conflict) {
    await runOp("paste_group", { project, storage });
    return;
  }
  pgState = { project, storage };
  $("pg-project").textContent = project;
  $("pg-group").value = preview.group_name;
  $("pg-list").innerHTML = preview.nodes
    .map((n) => {
      const kind = n.kind === "table" ? "📊 Table" : n.kind === "constant" ? "📋 Const" : "🔢 Enum";
      return `<div class="pg-row"><span class="pg-kind">${kind}</span><span class="pg-old" title="${escapeHtml(
        n.old_name,
      )}">${escapeHtml(n.old_name)}</span><span class="pg-arrow">→</span><input class="input" data-pg-old="${escapeHtml(
        n.old_name,
      )}" data-pg-kind="${escapeHtml(n.kind)}" value="${escapeHtml(n.new_name)}" />${
        n.conflict ? '<span class="pg-tag">重名</span>' : ""
      }</div>`;
    })
    .join("");
  $("pg-error").hidden = true;
  $("paste-group-dialog").hidden = false;
}

async function submitPasteGroup() {
  if (!pgState) return;
  const group = $("pg-group").value.trim();
  if (!group) return showFormError($("pg-error"), "组名不能为空");
  const nodes = [...document.querySelectorAll("#pg-list input[data-pg-old]")].map((i) => ({
    kind: i.dataset.pgKind,
    old_name: i.dataset.pgOld,
    new_name: i.value.trim(),
    conflict: false,
  }));
  if (nodes.some((n) => !n.new_name)) return showFormError($("pg-error"), "子项名称不能为空");
  try {
    const snap = await invoke("paste_group_named", {
      project: pgState.project,
      storage: pgState.storage,
      groupName: group,
      nodes,
    });
    applySnapshot(snap);
    state.expandedProjects.add(pgState.project);
    state.expandedGroups.add(`${pgState.project}|${group}`);
    renderTree();
    $("paste-group-dialog").hidden = true;
    toast(`已粘贴 Group：${group}`, "ok");
    pgState = null;
  } catch (e) {
    showFormError($("pg-error"), String(e));
  }
}

function initPasteGroupDialog() {
  $("pg-cancel").addEventListener("click", () => {
    $("paste-group-dialog").hidden = true;
    pgState = null;
  });
  $("pg-ok").addEventListener("click", submitPasteGroup);
}

async function saveProject(project) {
  await runOp("select_node", { project, kind: "project" });
  await runOp("save_workspace", {});
  toast("项目已保存", "ok");
}

async function importTblFlow(project, group) {
  let target = group;
  if (!target) {
    const r = await openModal({ title: "导入 .tbl 到 Group", placeholder: "组名" });
    if (!r || !r.value) return;
    target = r.value;
  }
  const paths = await pickPaths("选择 .tbl 文件", "tbl");
  if (!paths || !paths.length) return;
  await runOp("import_tbl", { project, group: target, paths });
  toast("已导入 .tbl", "ok");
}

async function importXlsxFlow(project) {
  const path = await pickPath("选择 Excel 工作簿", "xlsx");
  if (!path) return;
  await runOp("import_xlsx_group", { project, path });
  toast("已导入 Excel Group", "ok");
}

async function excelEdit(project, group) {
  if (!project) return toast("没有活跃项目", "error");
  try {
    await invoke("excel_begin_edit", { project, group });
    toast(`已用 Excel 打开 ${group}（编辑后自动回写内存，保存后落盘）`, "ok");
  } catch (e) {
    toast(String(e), "error");
  }
}

/** 打开内置 Univer 编辑窗口；一个窗口 = 一个组，可同时开多个。 */
async function univerEdit(project, group) {
  if (!project) return toast("没有活跃项目", "error");
  if (!group) return toast("先选择一个分组", "error");
  try {
    await invoke("open_univer_editor", { project, group });
  } catch (e) {
    toast(String(e), "error");
  }
}

function projectMenu(t) {
  const row = state.snapshot?.tree.find((r) => r.kind === "project" && r.project === t.project);
  if (row?.closed) {
    return [
      { label: "打开", action: () => selectTree("project", t.project) },
      {
        label: "打开所在目录",
        action: () =>
          invoke("open_project_dir", { project: t.project }).catch((e) => toast(String(e), "error")),
      },
      {
        label: "项目设置…",
        action: async () => {
          await runOp("open_project", { project: t.project });
          await openProjectSettings(t.project);
        },
      },
      { sep: true },
      { label: "删除", danger: true, action: () => deleteProject(t.project) },
    ];
  }
  const st = status();
  return [
    { label: "保存", action: () => saveProject(t.project) },
    { label: "导出…", action: () => openExportDialog() },
    { label: "导出 Schema…", action: () => openSchemaDialog("export") },
    { label: "另存为模板…", action: () => openSchemaDialog("template") },
    { label: "合并 Schema…", action: () => openSchemaDialog("import") },
    { sep: true },
    { label: "新建 Group", action: () => newGroup(t.project, false) },
    { label: "新建 Excel Group", action: () => newGroup(t.project, true) },
    { label: "导入 .tbl 文件…", action: () => importTblFlow(t.project) },
    { label: "导入 Excel Group…", action: () => importXlsxFlow(t.project) },
    { label: "导入任意 Excel（映射）…", action: () => openExcelMap() },
    { label: "克隆项目…", action: () => cloneProject(t.project) },
    { sep: true },
    { label: "粘贴为 .tbl Group", disabled: !st.has_group_clipboard, action: () => pasteGroupFmt(t.project, "tbl") },
    { label: "粘贴为 Excel Group", disabled: !st.has_group_clipboard, action: () => pasteGroupFmt(t.project, "xlsx") },
    { sep: true },
    { label: "项目设置…", action: () => openProjectSettings(t.project) },
    { label: "删除…", danger: true, action: () => deleteProject(t.project) },
    { sep: true },
    { label: "关闭", action: () => closeProject(t.project) },
    {
      label: "打开所在目录",
      action: () =>
        invoke("open_project_dir", { project: t.project }).catch((e) => toast(String(e), "error")),
    },
  ];
}

function groupMenu(t) {
  const st = status();
  const locked = groupLocked(t.project, t.group);
  const row = state.snapshot?.tree.find(
    (r) => r.kind === "group" && r.project === t.project && r.group === t.group,
  );
  const xlsx = !!row?.xlsx;
  const items = [
    { label: "新建 Table（v3）", disabled: locked, action: () => newNode(t.project, t.group, "table", "v3") },
    { label: "新建 Table（v2）", disabled: locked, action: () => newNode(t.project, t.group, "table", "v2") },
    { label: "新建 Constant", disabled: locked, action: () => newNode(t.project, t.group, "constant") },
    { label: "新建 Enum", disabled: locked, action: () => newNode(t.project, t.group, "enum") },
    { sep: true },
    {
      label: "Univer 编辑",
      // 已经在这个组的 Univer 窗口里打开时不重复打开（主界面只读，点也只会聚焦）。
      disabled: locked,
      action: () => univerEdit(t.project, t.group),
    },
    {
      label: xlsx ? "用 Excel 打开（原文件）" : "用 Excel 打开（整组）",
      disabled: locked,
      action: () =>
        xlsx
          ? invoke("open_xlsx_book", { project: t.project, group: t.group }).catch((e) =>
              toast(String(e), "error"),
            )
          : excelEdit(t.project, t.group),
    },
    { label: "导出 Excel…", action: () => openExcelDialog("export", t.group) },
    { label: "导入 Excel…", disabled: locked, action: () => openExcelDialog("import", t.group) },
    { sep: true },
    { label: "复制 Group", action: () => runOp("copy_group", { project: t.project, group: t.group }) },
    { label: "粘贴节点", disabled: locked || !st.has_node_clipboard, action: () => pasteNodeFmt(t.project, t.group) },
    { label: "粘贴为 v3", disabled: locked || !st.node_clipboard_is_table, action: () => pasteNodeFmt(t.project, t.group, "v3") },
    { label: "粘贴为 v2", disabled: locked || !st.node_clipboard_is_table, action: () => pasteNodeFmt(t.project, t.group, "v2") },
    { label: "粘贴为 .tbl Group", disabled: locked || !st.has_group_clipboard, action: () => pasteGroupFmt(t.project, "tbl") },
    { label: "粘贴为 Excel Group", disabled: locked || !st.has_group_clipboard, action: () => pasteGroupFmt(t.project, "xlsx") },
    { sep: true },
    { label: "重命名", disabled: locked, action: () => renameGroup(t.project, t.group) },
    { label: "删除", danger: true, disabled: locked, action: () => deleteGroup(t.project, t.group) },
  ];
  if (xlsx) {
    items.push({
      label: "重新加载",
      action: () => runOp("reload_group", { project: t.project, group: t.group }),
    });
  }
  if ((status().excel_editing || []).includes(t.group)) {
    items.push({
      label: "结束 Excel 编辑",
      action: () => runOp("excel_end_edit", { project: t.project, group: t.group }),
    });
    items.push({
      label: "放弃 Excel 改动",
      danger: true,
      action: async () => {
        const ok = await openModal({
          title: "放弃 Excel 改动",
          message: `结束 ${t.group} 的 Excel 会话，且不把工作簿改动合并回内存？`,
          input: false,
        });
        if (ok) await runOp("excel_abandon", { project: t.project, group: t.group });
      },
    });
  }
  return items;
}

function nodeMenu(t) {
  const st = status();
  const locked = groupLocked(t.project, t.group);
  const items = [];
  if (t.kind !== "constant") {
    items.push({ label: "校验规则…", disabled: locked, action: () => openRulesEditor() });
    items.push({ sep: true });
  }
  items.push(
    { label: "复制", action: () => runOp("copy_node", { project: t.project, group: t.group, name: t.name, kind: t.kind }) },
    { label: "粘贴节点", disabled: locked || !st.has_node_clipboard, action: () => pasteNodeFmt(t.project, t.group) },
    { label: "粘贴为 v3", disabled: locked || !st.node_clipboard_is_table, action: () => pasteNodeFmt(t.project, t.group, "v3") },
    { label: "粘贴为 v2", disabled: locked || !st.node_clipboard_is_table, action: () => pasteNodeFmt(t.project, t.group, "v2") },
    { sep: true },
    { label: "重命名", disabled: locked, action: () => renameNode(t) },
    { label: "删除", danger: true, disabled: locked, action: () => deleteNode(t) },
  );
  return items;
}

async function newGroup(project, xlsx) {
  let name = "";
  let useXlsx = !!xlsx;
  if (xlsx === undefined) {
    const r = await openModal({
      title: "新建 Group",
      placeholder: "例如 hero",
      xlsx: true,
      validate: (v) => invoke("validate_name", { project, group: null, kind: "group", name: v, oldName: null }),
    });
    if (!r || !r.value) return;
    name = r.value;
    useXlsx = r.xlsx;
  } else {
    const r = await openModal({
      title: useXlsx ? "新建 Excel Group" : "新建 Group",
      placeholder: "例如 hero",
      validate: (v) => invoke("validate_name", { project, group: null, kind: "group", name: v, oldName: null }),
    });
    if (!r || !r.value) return;
    name = r.value;
  }
  await runOp("new_group", { project, name, xlsx: useXlsx });
  state.expandedProjects.add(project);
  renderTree();
}

async function cloneProject(source) {
  const idr = await openModal({
    title: "复制项目 — 新 ID",
    placeholder: "[a-z0-9_-]{1,32}",
    value: `${source}_copy`,
  });
  if (!idr || !idr.value) return;
  const nameR = await openModal({ title: "复制项目 — 显示名称", value: idr.value });
  if (!nameR) return;
  await runOp("clone_project", {
    source,
    newId: idr.value,
    newName: nameR.value || idr.value,
  });
  state.expandedProjects.add(idr.value);
  toast("项目已复制（未保存）", "ok");
}

async function closeProject(project) {
  const row = state.snapshot?.tree.find((r) => r.kind === "project" && r.project === project);
  if (row?.mark) {
    const ok = await openModal({
      title: "关闭项目",
      message: `项目 ${project} 有未保存的改动，关闭会丢弃这些改动。确定关闭？`,
      input: false,
    });
    if (!ok) return;
  }
  await runOp("close_project", { project });
  state.expandedProjects.delete(project);
  renderTree();
  toast("项目已关闭", "ok");
}

async function deleteProject(project) {
  const r = await openModal({
    title: "删除项目",
    message: `删除会移除磁盘目录 projects/${project}/，不可恢复。输入项目 ID「${project}」确认：`,
    placeholder: project,
  });
  if (!r || r.value !== project) return;
  await runOp("delete_project", { project });
  state.expandedProjects.delete(project);
  renderTree();
  toast("项目已删除", "ok");
}

async function newNode(project, group, kind, fmt) {
  const label =
    kind === "table"
      ? fmt === "v2"
        ? "Table（v2）"
        : "Table（v3）"
      : kind === "constant"
        ? "Constant"
        : "Enum";
  const r = await openModal({
    title: `新建 ${label}`,
    placeholder: "PascalCase 名称",
    validate: (v) => invoke("validate_name", { project, group, kind, name: v, oldName: null }),
  });
  if (!r || !r.value) return;
  await runOp("new_node", {
    project,
    group,
    kind,
    name: r.value,
    fmt: fmt || null,
  });
  state.expandedProjects.add(project);
  state.expandedGroups.add(`${project}|${group}`);
  selectTree(kind, project, group, r.value);
}

async function renameGroup(project, oldName) {
  const r = await openModal({
    title: "重命名 Group",
    value: oldName,
    validate: (v) => invoke("validate_name", { project, group: null, kind: "group", name: v, oldName }),
  });
  if (!r || !r.value || r.value === oldName) return;
  await runOp("rename_group", { project, oldName, newName: r.value });
  state.expandedGroups.delete(`${project}|${oldName}`);
  state.expandedGroups.add(`${project}|${r.value}`);
  renderTree();
}

async function renameNode(t) {
  const r = await openModal({
    title: "重命名节点",
    value: t.name,
    validate: (v) => invoke("validate_name", { project: t.project, group: t.group, kind: t.kind, name: v, oldName: t.name }),
  });
  if (!r || !r.value || r.value === t.name) return;
  await runOp("rename_node", { project: t.project, group: t.group, oldName: t.name, newName: r.value });
  if (state.selected && state.selected.name === t.name && state.selected.group === t.group) {
    state.selected.name = r.value;
  }
}

async function deleteGroup(project, group) {
  const ok = await openModal({
    title: "删除 Group",
    message: `确定删除 ${group}？（保存后才真正从磁盘移除）`,
    input: false,
  });
  if (!ok) return;
  await runOp("delete_group", { project, group });
  state.expandedGroups.delete(`${project}|${group}`);
  if (state.selected && state.selected.group === group) {
    state.selected = { project, kind: "project", group: null, name: null };
  }
  renderTree();
}

async function deleteNode(t) {
  const ok = await openModal({
    title: `删除 ${t.kind === "table" ? "Table" : t.kind === "constant" ? "Constant" : "Enum"}`,
    message: `确定删除 ${t.group}/${t.name}？保存后才真正落盘。`,
    input: false,
  });
  if (!ok) return;
  await runOp("delete_node", { project: t.project, group: t.group, name: t.name });
  if (state.selected && state.selected.name === t.name && state.selected.group === t.group) {
    state.selected = { project: t.project, kind: "group", group: t.group, name: null };
  }
  renderTree();
}

function gridMenu(ctx) {
  const b = grid.selectionBounds();
  const f = grid.focus;
  const items = [];
  if (ctx.shape === "cell") {
    const kind = grid.node?.kinds?.[f.c];
    const pick =
      kind === "ref" ? "选择引用…" : kind === "type" ? "选择类型…" : kind === "export" ? "选择导出…" : null;
    if (pick) {
      items.push({ label: pick, action: () => grid.beginCellEdit(f.r, f.c) });
      items.push({ sep: true });
    }
    items.push({ label: "复制", action: () => grid.copySelection(false) });
    items.push({ label: "剪切", action: () => grid.copySelection(true) });
    items.push({ label: "粘贴", action: () => grid.pasteClipboard() });
    items.push({ label: "清空内容", action: () => grid.clearSelection() });
    items.push({ sep: true });
  }
  if (ctx.shape === "cell" || ctx.shape === "row") {
    const r0 = b ? b.r0 : f.r;
    const r1 = b ? b.r1 : f.r;
    items.push({ label: "在上方插入行", action: () => runOp("insert_row", { at: r0 }) });
    items.push({ label: "在下方插入行", action: () => runOp("insert_row", { at: r1 + 1 }) });
    items.push({ label: "删除行", danger: true, action: () => runOp("delete_rows", { start: r0, end: r1 }) });
    items.push({ sep: true });
  }
  if (ctx.shape === "cell" || ctx.shape === "col") {
    const c = f.c;
    items.push({
      label: "在左侧插入列",
      action: () => runOp("insert_column", { at: c }).then(() => grid.revealColumn(c)),
    });
    items.push({
      label: "在右侧插入列",
      action: () => runOp("insert_column", { at: c + 1 }).then(() => grid.revealColumn(c + 1)),
    });
    items.push({ label: "删除列", danger: true, action: () => runOp("delete_column", { at: c }) });
  }
  if (ctx.shape === "blank") items.push({ label: "粘贴", action: () => grid.pasteClipboard() });
  if (grid.locked) {
    for (const it of items) {
      if (!it.sep && it.label !== "复制" && it.label !== "剪切") it.disabled = true;
    }
  }
  return items;
}

function initContextMenus() {
  $("tree-list").addEventListener("contextmenu", (e) => {
    e.preventDefault();
    hideContextMenu();
    const item = e.target.closest(".tree-item");
    if (!item) {
      const active = status().active_project;
      const items = [{ label: "新建项目", action: () => openCreateProject() }];
      if (active) {
        items.push(
          { label: "新建 Group", action: () => newGroup(active, false) },
          { label: "新建 Excel Group", action: () => newGroup(active, true) },
        );
      }
      showContextMenu(e.clientX, e.clientY, items);
      return;
    }
    const t = decodeTreeKey(item.dataset.key);
    if (t.kind === "project") {
      selectTree("project", t.project);
      showContextMenu(e.clientX, e.clientY, projectMenu(t));
    } else if (t.kind === "group") {
      selectTree("group", t.project, t.group);
      showContextMenu(e.clientX, e.clientY, groupMenu(t));
    } else {
      selectTree(t.kind, t.project, t.group, t.name);
      showContextMenu(e.clientX, e.clientY, nodeMenu(t));
    }
  });

  document.addEventListener("pointerdown", (e) => {
    if (!e.target.closest("#ctx-menu")) hideContextMenu();
  });
  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest("#ctx-menu") && !e.defaultPrevented) hideContextMenu();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideContextMenu();
  });
}

// ── 新建项目 / 项目设置 ─────────────────────────────────────
let templatesCache = null;

async function ensureTemplates() {
  if (!templatesCache) templatesCache = await invoke("list_templates");
  return templatesCache;
}

function showFormError(el, msg) {
  el.textContent = msg;
  el.hidden = false;
}

const cpState = {
  tab: "empty",
  preview: null,
  selected: new Set(),
  source: "builtin",
  templateId: "",
  templates: [],
  bound: false,
  // 哪些身份字段是「来源自动填的」：用户手改后置 false，再来新预览就不再覆盖。
  auto: {},
};

/** 切换来源 tab 时清空上一个来源的预览、勾选和身份字段，避免旧数据被带进新来源。 */
function resetCpSource() {
  cpState.preview = null;
  cpState.selected = new Set();
  cpState.templateId = "";
  cpState.source = "builtin";
  cpState.auto = {};
  for (const id of ["cp-id", "cp-name", "cp-category", "cp-version", "cp-file", "cp-tpl-search"]) {
    $(id).value = "";
  }
  $("cp-file-sections").innerHTML = "";
  $("cp-tpl-sections").innerHTML = "";
  $("cp-preset").checked = false;
  $("cp-error").hidden = true;
  document
    .querySelectorAll("#cp-sources [data-cpsource]")
    .forEach((b) => b.classList.toggle("active", b.dataset.cpsource === "builtin"));
  updateCpPreview();
}

function setCpTab(tab) {
  if (cpState.tab !== tab) resetCpSource();
  cpState.tab = tab;
  document
    .querySelectorAll("#cp-tabs .ps-tab")
    .forEach((t) => t.classList.toggle("active", t.dataset.cptab === tab));
  document.querySelectorAll("#create-project .ps-pane").forEach((p) => {
    p.hidden = p.dataset.cppane !== tab;
  });
  if (tab === "template") renderCpTemplateList();
}

function updateCpPreview() {
  $("cp-preview").textContent = `目录预览：projects/${$("cp-id").value.trim() || "<id>"}/`;
}

function renderCpSections(containerId, preview) {
  const el = $(containerId);
  if (!preview) {
    el.innerHTML = "";
    return;
  }
  const items = preview.sections || [];
  const all =
    items.length > 0 &&
    items.every((s) => cpState.selected.has(`${s.group}/${s.name}`));
  el.innerHTML =
    `<div class="cp-sec-head"><label class="check"><input type="checkbox" data-sec-all="1" ${
      all ? "checked" : ""
    }/> 全选</label><span>已选 ${cpState.selected.size}/${items.length} 个 section</span></div>` +
    items
      .map((s) => {
        const key = `${s.group}/${s.name}`;
        return `<label class="cp-sec"><input type="checkbox" data-sec="${escapeHtml(
          key,
        )}" ${cpState.selected.has(key) ? "checked" : ""}/> ${escapeHtml(
          key,
        )}<span class="cp-mode">${escapeHtml(s.mode)}</span></label>`;
      })
      .join("");
}

function bindSectionContainer(containerId) {
  $(containerId).addEventListener("change", (e) => {
    const all = e.target.closest("[data-sec-all]");
    if (all) {
      const keys = (cpState.preview?.sections || []).map((s) => `${s.group}/${s.name}`);
      cpState.selected = new Set(all.checked ? keys : []);
      renderCpSections(containerId, cpState.preview);
      return;
    }
    const sec = e.target.closest("[data-sec]");
    if (!sec) return;
    if (sec.checked) cpState.selected.add(sec.dataset.sec);
    else cpState.selected.delete(sec.dataset.sec);
    renderCpSections(containerId, cpState.preview);
  });
}

/** 用来源预览填项目身份；只覆盖「空字段」或「上一次自动填过、用户没改」的字段。 */
function fillCpIdentity(preview) {
  const fields = [
    ["id", $("cp-id"), (preview.id || "").toLowerCase().replace(/[^a-z0-9_-]/g, "")],
    ["name", $("cp-name"), preview.name || ""],
    ["category", $("cp-category"), preview.category || ""],
    ["version", $("cp-version"), preview.version || ""],
  ];
  for (const [key, el, value] of fields) {
    if (!el.value || cpState.auto[key]) {
      el.value = value;
      cpState.auto[key] = true;
    }
  }
}

function applyPreview(preview) {
  cpState.preview = preview;
  cpState.selected = new Set((preview.sections || []).map((s) => `${s.group}/${s.name}`));
  fillCpIdentity(preview);
  $("cp-preset").checked = !!preview.has_preset;
  renderCpSections("cp-file-sections", preview);
  renderCpSections("cp-tpl-sections", preview);
  updateCpPreview();
}

async function loadFilePreview() {
  const path = $("cp-file").value.trim();
  if (!path) return;
  try {
    const preview = await invoke("schema_preview", { path });
    applyPreview(preview);
    $("cp-error").hidden = true;
  } catch (e) {
    showFormError($("cp-error"), String(e));
  }
}

function renderCpTemplateList() {
  const q = $("cp-tpl-search").value.trim().toLowerCase();
  const list = cpState.templates.filter(
    (t) => t.source === cpState.source && (!q || `${t.id} ${t.name}`.toLowerCase().includes(q)),
  );
  $("cp-tpl-list").innerHTML = list.length
    ? list
        .map(
          (t) =>
            `<div class="cp-tpl${t.id === cpState.templateId ? " active" : ""}" data-tpl="${escapeHtml(
              t.id,
            )}"><div>${escapeHtml(t.name)}</div><div class="meta">ID: ${escapeHtml(
              t.id,
            )} · ${t.sections} 个 section · ${escapeHtml(t.source)} · v${escapeHtml(
              t.version || "-",
            )}</div></div>`,
        )
        .join("")
    : '<div class="sys-empty">该来源没有模板</div>';
}

async function loadTemplatePreview(id) {
  cpState.templateId = id;
  renderCpTemplateList();
  try {
    const preview = await invoke("template_preview", { templateId: id });
    applyPreview(preview);
    $("cp-error").hidden = true;
  } catch (e) {
    showFormError($("cp-error"), String(e));
  }
}

async function openCreateProject() {
  cpState.tab = "empty";
  // 每次打开都从空白开始：上一轮的来源、预览、勾选、身份字段和模板来源按钮全部复位。
  resetCpSource();
  $("cp-open-now").checked = true;
  setCpTab("empty");
  $("create-project").hidden = false;
  $("cp-id").focus();
  try {
    cpState.templates = await ensureTemplates();
  } catch (e) {
    cpState.templates = [];
    addLocalLog(2, String(e));
  }
  if (!cpState.bound) {
    bindSectionContainer("cp-file-sections");
    bindSectionContainer("cp-tpl-sections");
    cpState.bound = true;
  }
  renderCpTemplateList();
}

async function submitCreateProject() {
  const id = $("cp-id").value.trim();
  const err = $("cp-error");
  err.hidden = true;
  if (!id) return showFormError(err, "项目 ID 不能为空");
  if (!$("cp-name").value.trim()) return showFormError(err, "项目名称不能为空");
  if (cpState.tab !== "empty" && !cpState.preview) {
    return showFormError(err, "请先选择文件 / 模板");
  }
  if (cpState.tab !== "empty" && cpState.selected.size === 0) {
    return showFormError(err, "至少选择一个 section");
  }
  const req = {
    source: cpState.tab,
    id,
    name: $("cp-name").value.trim(),
    category: $("cp-category").value.trim(),
    version: $("cp-version").value.trim(),
    path: cpState.tab === "file" ? $("cp-file").value.trim() : null,
    template_id: cpState.tab === "template" ? cpState.templateId : null,
    sections: [...cpState.selected],
    with_preset: $("cp-preset").checked,
    open_now: $("cp-open-now").checked,
  };
  try {
    const snap = await invoke("create_project", { request: req });
    $("create-project").hidden = true;
    state.expandedProjects.add(id);
    if (req.open_now) {
      state.selected = { project: id, kind: "project", group: null, name: null };
    }
    applySnapshot(snap);
    renderTree();
    toast(`已创建项目 ${id}（保存后落盘）`, "ok");
  } catch (e) {
    showFormError(err, String(e));
  }
}

const EXPORT_TEXT_FIELDS = {
  encoding: "ps-enc",
  line_ending: "ps-eol",
  json_empty_as: "ps-json-empty",
  xml_empty_as: "ps-xml-empty",
  server_data_output: "ps-server-data",
  java_package: "ps-java-pkg",
  java_output: "ps-java-out",
  go_package: "ps-go-pkg",
  go_output: "ps-go-out",
  cpp_namespace: "ps-cpp-ns",
  cpp_output: "ps-cpp-out",
  dotnet_namespace: "ps-dotnet-ns",
  dotnet_output: "ps-dotnet-out",
  server_ts_output: "ps-ts-server-out",
  lua_output: "ps-lua-out",
  gdscript_output: "ps-gdscript-out",
  client_ts_output: "ps-ts-client-out",
  unity_namespace: "ps-unity-ns",
  unity_output: "ps-unity-out",
  godot_namespace: "ps-godot-ns",
  godot_output: "ps-godot-out",
};

const EXPORT_BOOL_FIELDS = {
  java_lombok: "ps-java-lombok",
  java_custom_regions: "ps-java-regions",
  go_custom_regions: "ps-go-regions",
  cpp_custom_regions: "ps-cpp-regions",
  dotnet_custom_regions: "ps-dotnet-regions",
  unity_custom_regions: "ps-unity-regions",
  godot_custom_regions: "ps-godot-regions",
};

function setPsTab(tab) {
  document.querySelectorAll("#project-settings .ps-tab").forEach((t) => t.classList.toggle("active", t.dataset.pstab === tab));
  document.querySelectorAll("#project-settings .ps-pane").forEach((p) => {
    p.hidden = p.dataset.pane !== tab;
  });
  // 「应用模板」只在分隔符 / 导出页出现（与 Slint 版一致）。
  $("ps-apply-template").style.display = tab === "sep" || tab === "export" ? "" : "none";
  refreshPsModified();
}

function psExportEntries() {
  return [...Object.entries(EXPORT_TEXT_FIELDS), ...Object.entries(EXPORT_BOOL_FIELDS)];
}

/** 给每个导出字段补一个 ↺ 按钮（只补一次）。 */
function ensurePsRevertButtons() {
  for (const [, id] of psExportEntries()) {
    const el = $(id);
    if (!el || el.dataset.psRevDone) continue;
    el.dataset.psRevDone = "1";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ps-rev";
    btn.dataset.psRevExport = id;
    btn.title = "还原到上次保存";
    btn.textContent = "↺";
    const label = el.closest("label.check");
    if (label) label.insertAdjacentElement("afterend", btn);
    else (el.closest(".form-row") || el.parentElement).appendChild(btn);
  }
}

function setPsExportValue(key, value) {
  const id = EXPORT_TEXT_FIELDS[key] || EXPORT_BOOL_FIELDS[key];
  if (!id) return;
  if (EXPORT_BOOL_FIELDS[key]) $(id).checked = !!value;
  else $(id).value = value ?? "";
}

function markPs(el, modified) {
  el.classList.toggle("ps-mod", modified);
}

/** 按「上次保存值」重算所有字段的已修改高亮与 ↺ 可用态。 */
function refreshPsModified() {
  const orig = psState.original;
  if (!orig || $("project-settings").hidden) return;
  let any = false;
  for (const key of ["name", "category", "version"]) {
    const el = $(`ps-${key}`);
    const mod = el.value !== (orig[key] || "");
    markPs(el, mod);
    any = any || mod;
    const rev = document.querySelector(`[data-ps-rev="${key}"]`);
    if (rev) rev.disabled = !mod;
  }
  document.querySelectorAll("#ps-seps input[data-sep-key]").forEach((input) => {
    const expected = orig.seps[input.dataset.sepKey] ?? "";
    const mod = input.value !== expected;
    markPs(input, mod);
    any = any || mod;
    const rev = document.querySelector(`[data-sep-rev="${CSS.escape(input.dataset.sepKey)}"]`);
    if (rev) rev.disabled = !mod;
  });
  for (const [key, id] of psExportEntries()) {
    const el = $(id);
    const isBool = !!EXPORT_BOOL_FIELDS[key];
    const expected = orig.export[key];
    const mod = isBool ? el.checked !== !!expected : el.value !== String(expected ?? "");
    markPs(el, mod);
    if (isBool) markPs(el.closest("label.check") || el, mod);
    any = any || mod;
    const rev = document.querySelector(`[data-ps-rev-export="${id}"]`);
    if (rev) rev.disabled = !mod;
  }
  $("ps-revert-all").disabled = !any;
}

/** 全部还原：身份 + 分隔符 + 导出 回到上次保存。 */
function applyPsOriginal() {
  const orig = psState.original;
  if (!orig) return;
  $("ps-name").value = orig.name || "";
  $("ps-category").value = orig.category || "";
  $("ps-version").value = orig.version || "";
  document.querySelectorAll("#ps-seps input[data-sep-key]").forEach((input) => {
    input.value = orig.seps[input.dataset.sepKey] ?? "";
  });
  for (const [key] of psExportEntries()) setPsExportValue(key, orig.export[key]);
  refreshPsModified();
}

/** 应用模板：分隔符 / 导出 回到硬编码默认值（不碰身份）。 */
function applyPsDefaults() {
  const defs = psState.defaults;
  if (!defs) return;
  document.querySelectorAll("#ps-seps input[data-sep-key]").forEach((input) => {
    input.value = defs.seps[input.dataset.sepKey] ?? "";
  });
  for (const [key] of psExportEntries()) setPsExportValue(key, defs.export[key]);
  refreshPsModified();
}

// 项目设置的「已修改」参照：上次保存值（original）与硬编码默认值（defaults）。
let psState = { original: null, defaults: null };

function renderSeparators(items) {
  state.psSeparators = items;
  $("ps-seps").innerHTML = items
    .map(
      (it) =>
        `<div class="sep-row" data-sep-row="${escapeHtml(it.key)}"><label title="${escapeHtml(
          it.key,
        )}">${escapeHtml(it.key)}</label>` +
        `<input data-sep-key="${escapeHtml(it.key)}" value="${escapeHtml(it.value)}" placeholder="${escapeHtml(
          it.default,
        )}" /><button class="ps-rev" data-sep-rev="${escapeHtml(
          it.key,
        )}" title="还原到上次保存">↺</button></div>`,
    )
    .join("");
}

function setExportFields(e) {
  for (const [key, id] of Object.entries(EXPORT_TEXT_FIELDS)) {
    $(id).value = e[key] ?? "";
  }
  for (const [key, id] of Object.entries(EXPORT_BOOL_FIELDS)) {
    $(id).checked = !!e[key];
  }
}

function collectExportFields() {
  const out = {};
  for (const [key, id] of Object.entries(EXPORT_TEXT_FIELDS)) out[key] = $(id).value.trim();
  for (const [key, id] of Object.entries(EXPORT_BOOL_FIELDS)) out[key] = $(id).checked;
  return out;
}

async function openProjectSettings(project) {
  try {
    const s = await invoke("project_settings", { project });
    psState = {
      original: {
        name: s.original_name || "",
        category: s.original_category || "",
        version: s.original_version || "",
        seps: Object.fromEntries((s.separators || []).map((x) => [x.key, x.original ?? ""])),
        export: s.original_export || {},
      },
      defaults: {
        seps: Object.fromEntries((s.separators || []).map((x) => [x.key, x.default ?? ""])),
        export: s.default_export || {},
      },
    };
    $("ps-id").value = s.id;
    $("ps-name").value = s.name;
    $("ps-category").value = s.category;
    $("ps-version").value = s.version;
    renderSeparators(s.separators || []);
    setExportFields(s.export_settings || {});
    ensurePsRevertButtons();
    const v = s.validate || {};
    $("ps-val-error").value = v.error || "block";
    $("ps-val-warning").value = v.warning || "report";
    $("ps-val-info").value = v.info || "ignore";
    $("ps-error").hidden = true;
    $("project-settings").hidden = false;
    setPsTab("identity");
    refreshPsModified();
    $("ps-name").focus();
  } catch (e) {
    addLocalLog(2, String(e));
    toast(String(e), "error");
  }
}

async function submitProjectSettings() {
  const project = $("ps-id").value;
  const err = $("ps-error");
  err.hidden = true;
  const separators = [...document.querySelectorAll("#ps-seps input[data-sep-key]")].map((i) => ({
    key: i.dataset.sepKey,
    value: i.value,
  }));
  try {
    const snap = await invoke("save_project_settings", {
      project,
      name: $("ps-name").value.trim(),
      category: $("ps-category").value.trim(),
      version: $("ps-version").value.trim(),
      separators,
      exportSettings: collectExportFields(),
      validate: {
        error: $("ps-val-error").value,
        warning: $("ps-val-warning").value,
        info: $("ps-val-info").value,
      },
    });
    $("project-settings").hidden = true;
    applySnapshot(snap);
    addLocalLog(0, `已更新项目设置 ${project}`);
  } catch (e) {
    showFormError(err, String(e));
  }
}

function initProjectDialogs() {
  document.querySelectorAll("#cp-tabs .ps-tab").forEach((t) =>
    t.addEventListener("click", () => setCpTab(t.dataset.cptab)),
  );
  $("cp-sources").addEventListener("click", (e) => {
    const b = e.target.closest("[data-cpsource]");
    if (!b || b.disabled) return;
    cpState.source = b.dataset.cpsource;
    // 换来源 = 换一套模板：清掉已选模板、预览和身份，不要带着上一套数据。
    cpState.templateId = "";
    cpState.preview = null;
    cpState.selected = new Set();
    cpState.auto = {};
    $("cp-tpl-sections").innerHTML = "";
    $("cp-tpl-search").value = "";
    for (const id of ["cp-id", "cp-name", "cp-category", "cp-version"]) $(id).value = "";
    $("cp-preset").checked = false;
    $("cp-error").hidden = true;
    updateCpPreview();
    document
      .querySelectorAll("#cp-sources [data-cpsource]")
      .forEach((x) => x.classList.toggle("active", x === b));
    renderCpTemplateList();
  });
  $("cp-tpl-search").addEventListener("input", renderCpTemplateList);
  $("cp-tpl-list").addEventListener("click", (e) => {
    const it = e.target.closest("[data-tpl]");
    if (it) loadTemplatePreview(it.dataset.tpl);
  });
  $("cp-browse-file").addEventListener("click", async () => {
    const path = await invoke("pick_file", {
      title: "选择 .tblschema",
      extensions: ["tblschema"],
    }).catch(() => null);
    if (path) {
      $("cp-file").value = path;
      loadFilePreview();
    }
  });
  $("cp-file").addEventListener("change", loadFilePreview);
  for (const [id, key] of [
    ["cp-id", "id"],
    ["cp-name", "name"],
    ["cp-category", "category"],
    ["cp-version", "version"],
  ]) {
    $(id).addEventListener("input", () => {
      cpState.auto[key] = false;
      if (id === "cp-id") updateCpPreview();
    });
  }

  $("cp-cancel").addEventListener("click", () => ($("create-project").hidden = true));
  $("cp-ok").addEventListener("click", submitCreateProject);
  $("create-project").addEventListener("keydown", (e) => {
    if (e.key === "Escape") $("create-project").hidden = true;
    else if (e.key === "Enter" && e.target.tagName !== "TEXTAREA") {
      e.preventDefault();
      submitCreateProject();
    }
  });

  document.querySelectorAll("#project-settings .ps-tab").forEach((t) =>
    t.addEventListener("click", () => setPsTab(t.dataset.pstab)),
  );
  $("ps-revert-all").addEventListener("click", applyPsOriginal);
  $("ps-apply-template").addEventListener("click", applyPsDefaults);
  $("ps-ok").addEventListener("click", submitProjectSettings);
  // 字段级「已修改」高亮 + ↺ 单字段还原。
  $("project-settings").addEventListener("input", refreshPsModified);
  $("project-settings").addEventListener("change", refreshPsModified);
  $("project-settings").addEventListener("click", (e) => {
    const idRev = e.target.closest("[data-ps-rev]");
    if (idRev) {
      const key = idRev.dataset.psRev;
      if (psState.original) {
        $(`ps-${key}`).value = psState.original[key] || "";
        refreshPsModified();
      }
      return;
    }
    const sepRev = e.target.closest("[data-sep-rev]");
    if (sepRev && psState.original) {
      const input = document.querySelector(
        `#ps-seps input[data-sep-key="${CSS.escape(sepRev.dataset.sepRev)}"]`,
      );
      if (input) input.value = psState.original.seps[input.dataset.sepKey] ?? "";
      refreshPsModified();
      return;
    }
    const exRev = e.target.closest("[data-ps-rev-export]");
    if (exRev && psState.original) {
      const id = exRev.dataset.psRevExport;
      const key = psExportEntries().find(([, v]) => v === id)?.[0];
      if (key) {
        setPsExportValue(key, psState.original.export[key]);
        refreshPsModified();
      }
    }
  });
}

// ── 类型选择器（对齐 Slint TypeSelector）────────────────────
// 双 tab：数据类型（范式 + 参数槽 + 结果/示例/Java/Go/Lua 预览）/ 引用类型。
let tsState = null;
let tsPreviewSeq = 0;

function renderTs() {
  const s = tsState;
  if (!s) return;
  const box = $("type-selector");
  box.querySelectorAll("[data-tstab]").forEach((b) => {
    b.classList.toggle("active", b.dataset.tstab === s.tab);
    if (b.dataset.tstab === "ref") b.disabled = !s.info.allow_ref;
  });
  box.querySelectorAll("[data-tspane]").forEach((p) => {
    p.hidden = p.dataset.tspane !== s.tab;
  });
  box.querySelectorAll("[data-tsfilter]").forEach((b) => {
    b.classList.toggle("active", b.dataset.tsfilter === s.filter);
  });
  $("ts-paradigms").innerHTML = s.info.paradigms
    .map(
      (p) =>
        `<div class="ts-paradigm${p.index === s.paradigm ? " active" : ""}" data-tsparadigm="${p.index}">${escapeHtml(
          p.label,
        )}</div>`,
    )
    .join("");
  $("ts-ref-note").hidden = s.info.allow_ref;
  renderTsSlots();
  renderTsRefList();
  updateTsPreview();
}

function renderTsSlots() {
  const s = tsState;
  const p = s.info.paradigms[s.paradigm];
  const base = p.index === 0 ? s.info.base_all : s.info.base_nestable;
  $("ts-slots").innerHTML = p.slots
    .map((slot, i) => {
      const opts = (slot.is_map_key ? s.info.map_keys : base).concat(
        p.index === 0 ? [] : s.info.refs.map((r) => `@${r.name}`),
      );
      const cur = s.params[i] || "int";
      const list = opts.includes(cur) ? opts : [cur, ...opts];
      return `<div class="ts-slot"><label>${escapeHtml(slot.label)}</label>
        <select data-tsslot="${i}">${list
          .map(
            (v) =>
              `<option value="${escapeHtml(v)}"${v === cur ? " selected" : ""}>${escapeHtml(v)}</option>`,
          )
          .join("")}</select></div>`;
    })
    .join("");
}

function renderTsRefList() {
  const s = tsState;
  if (!s) return;
  const q = s.search.toLowerCase();
  $("ts-ref-search").value = s.search;
  const list = s.info.refs.filter(
    (r) => (s.filter === "all" || (s.filter === "table") === r.is_table) && (!q || r.name.toLowerCase().includes(q)),
  );
  $("ts-ref-list").innerHTML =
    list
      .map(
        (r) =>
          `<div class="ts-ref-item${r.name === s.refName ? " active" : ""}" data-tsref="${escapeHtml(
            r.name,
          )}"><span>${r.is_table ? "📊" : "🔢"}</span><span>${escapeHtml(r.name)}</span>
          <span class="meta">${r.is_table ? "table" : "enum"}</span></div>`,
      )
      .join("") || `<div class="ts-ref-item">（无匹配）</div>`;
}

async function updateTsPreview() {
  const s = tsState;
  if (!s) return;
  if (s.tab === "ref") {
    const name = s.refName;
    $("ts-result").textContent = name ? `@${name}` : "（未选择）";
    $("ts-example").textContent = name ? "（id）" : "—";
    $("ts-java").textContent = name ? "int" : "—";
    $("ts-go").textContent = name ? "int32" : "—";
    $("ts-lua").textContent = name ? "number" : "—";
    $("ts-error").hidden = true;
    $("ts-current").textContent = name ? `@${name}` : "";
    $("ts-ok").disabled = !name || !s.info.allow_ref;
    return;
  }
  const seq = ++tsPreviewSeq;
  try {
    const pv = await invoke("type_preview", { paradigm: s.paradigm, params: s.params });
    if (seq !== tsPreviewSeq || !tsState) return;
    s.preview = pv;
    $("ts-result").textContent = pv.type_string || "—";
    $("ts-example").textContent = pv.example || "—";
    $("ts-java").textContent = pv.java || "—";
    $("ts-go").textContent = pv.go || "—";
    $("ts-lua").textContent = pv.lua || "—";
    $("ts-error").hidden = pv.valid;
    $("ts-error").textContent = pv.error || "";
    $("ts-current").textContent = pv.valid ? pv.type_string : "";
    $("ts-ok").disabled = !pv.valid;
  } catch (e) {
    if (seq !== tsPreviewSeq) return;
    $("ts-error").hidden = false;
    $("ts-error").textContent = String(e);
    $("ts-ok").disabled = true;
  }
}

function closeTypeSelector(value) {
  if (!tsState) return;
  const resolve = tsState.resolve;
  tsState = null;
  $("type-selector").hidden = true;
  resolve?.(value);
}

function confirmTypeSelector() {
  const s = tsState;
  if (!s) return;
  if (s.tab === "ref") {
    if (!s.info.allow_ref || !s.refName) return;
    closeTypeSelector(`@${s.refName}`);
  } else if (s.preview?.valid) {
    closeTypeSelector(s.preview.type_string);
  }
}

/** 打开类型选择器；返回 Promise<string|null>。 */
async function openTypeSelector(target) {
  const sel = state.selected;
  if (!sel || !sel.name) return null;
  if (tsState) {
    const prev = tsState.resolve;
    tsState = null;
    $("type-selector").hidden = true;
    prev?.(null);
  }
  let info;
  try {
    info = await invoke("type_picker_info", {
      group: sel.group,
      name: sel.name,
      current: target.current || "",
    });
  } catch (e) {
    addLocalLog(2, String(e));
    return null;
  }
  tsState = {
    info,
    target,
    tab: info.ref_name && info.allow_ref ? "ref" : "data",
    paradigm: info.paradigm_index ?? 0,
    params: [...(info.params || [])],
    refName: info.ref_name || "",
    filter: "all",
    search: "",
    preview: null,
    resolve: null,
  };
  $("type-selector").hidden = false;
  renderTs();
  return new Promise((resolve) => {
    if (tsState) tsState.resolve = resolve;
  });
}

function initTypeSelector() {
  $("type-selector").addEventListener("click", (e) => {
    if (e.target.closest("#ts-cancel")) return closeTypeSelector(null);
    if (e.target.closest("#ts-ok")) return confirmTypeSelector();
    const tab = e.target.closest("[data-tstab]");
    if (tab) {
      if (tab.disabled) return;
      tsState.tab = tab.dataset.tstab;
      renderTs();
      return;
    }
    const para = e.target.closest("[data-tsparadigm]");
    if (para) {
      const idx = Number(para.dataset.tsparadigm);
      tsState.paradigm = idx;
      tsState.params = tsState.info.paradigms[idx].slots.map(() => "int");
      renderTs();
      return;
    }
    const ref = e.target.closest("[data-tsref]");
    if (ref) {
      tsState.refName = ref.dataset.tsref;
      renderTsRefList();
      updateTsPreview();
      return;
    }
    const chip = e.target.closest("[data-tsfilter]");
    if (chip) {
      tsState.filter = chip.dataset.tsfilter;
      renderTs();
    }
  });
  $("type-selector").addEventListener("change", (e) => {
    const slot = e.target.closest("[data-tsslot]");
    if (!slot || !tsState) return;
    tsState.params[Number(slot.dataset.tsslot)] = e.target.value;
    updateTsPreview();
  });
  $("ts-ref-search").addEventListener("input", (e) => {
    if (!tsState) return;
    tsState.search = e.target.value.trim();
    renderTsRefList();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("type-selector").hidden && $("modal").hidden) closeTypeSelector(null);
  });
}

// ── 校验规则编辑器 ─────────────────────────────────────────
//
// 对齐 Slint 版（@docs/16）与 rule-builder-v2 的交互：一条规则一张卡片，
// 卡片里是可增删的「组 / 验证器 / 条件」行；行的参数统一在底部 inspector 编辑。
// 草稿用草稿树保存，组内按连接符（and / or，AND 优先）组合；条件行保存时
// 与同组验证器合并成 `when(条件, 验证器)`，不会单独成规则。
const rulesState = {
  payload: null,
  scope: "column", // column | table
  target: "",
  tab: "custom",
  dirty: false,
  selected: null, // { rule, path: "0.1" }
  drafts: [], // [{ target, level, collapsed, draft }]
  pendingSwitch: null, // { scope, target } —— 有未保存改动时等待确认的切换
};

const LEAF_OPS = [
  ["eq", "等于"],
  ["ne", "不等于"],
  ["lt", "小于"],
  ["le", "小于等于"],
  ["gt", "大于"],
  ["ge", "大于等于"],
  ["in", "属于"],
  ["match", "匹配正则"],
  ["custom", "自定义"],
];

const PARAM_LABEL = {
  column: "列",
  text: "文本",
  regex: "正则",
  bound: "边界值",
  compare_op: "比较符",
  node_column: "节点.列",
};
const PARAM_PLACEHOLDER = {
  column: "列名",
  text: "文本",
  regex: "^...$",
  bound: "留空 = 不限",
  compare_op: "",
  node_column: "Node.column",
};

function fnByName(name) {
  return rulesState.payload?.functions.find((f) => f.name === name) || null;
}

function currentTarget() {
  return rulesState.scope === "table" ? "" : rulesState.target;
}

function firstColumn() {
  return rulesState.payload?.columns?.[0] || "";
}

function defaultArg(kind, boundIndex = 0) {
  if (kind === "column") return currentTarget() || firstColumn();
  if (kind === "compare_op") return "<=";
  if (kind === "bound") return boundIndex === 0 ? "0" : "";
  return "";
}

/** 列级作用域省略主列参数；返回真正展示 / 编辑的参数位。 */
function visibleParams(fn) {
  const out = [];
  if (!fn) return out;
  fn.params.forEach((kind, i) => {
    if (rulesState.scope === "column" && fn.primary_index === i) return;
    out.push({ kind, index: out.length });
  });
  return out;
}

function defaultArgs(fn) {
  if (!fn) return [];
  const out = [];
  let boundSeen = 0;
  fn.params.forEach((kind, i) => {
    if (rulesState.scope === "column" && fn.primary_index === i) return;
    out.push(defaultArg(kind, kind === "bound" ? boundSeen : 0));
    if (kind === "bound") boundSeen += 1;
  });
  if (fn.variadic) out.push(defaultArg(fn.variadic, boundSeen));
  return out;
}

function exprText(e) {
  if (!e) return "";
  if (e.kind === "Fun") return `${e.name}(${(e.args || []).join(", ")})`;
  if (e.kind === "When") return `when(${e.cond || ""}, ${exprText(e.inner)})`;
  return `${e.op}(${(e.members || []).map(exprText).join(", ")})`;
}

/** 按顶层分隔符切分；括号内、引号内的分隔符不切（and/or 嵌套条件依赖这一点）。 */
function splitTopLevel(s, sep) {
  const out = [];
  let cur = "";
  let quoted = false;
  let escaped = false;
  let depth = 0;
  for (const ch of s || "") {
    if (escaped) {
      cur += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\" && quoted) {
      cur += ch;
      escaped = true;
      continue;
    }
    if (ch === '"') {
      quoted = !quoted;
      cur += ch;
      continue;
    }
    if (!quoted) {
      if (ch === "(") depth += 1;
      else if (ch === ")") depth -= 1;
      else if (ch === sep && depth === 0) {
        out.push(cur.trim());
        cur = "";
        continue;
      }
    }
    cur += ch;
  }
  out.push(cur.trim());
  return out;
}

function stripQuotes(s) {
  const t = (s || "").trim();
  return t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1).replace(/\\"/g, '"') : t;
}

/** 条件文本 → 条件树（cmp / in / match / and / or / not / custom）。 */
function parseCondDeep(text) {
  const t = (text || "").trim();
  let m = /^(and|or)\((.*)\)$/.exec(t);
  if (m) {
    const parts = splitTopLevel(m[2], ",");
    return { kind: m[1], members: parts.map(parseCondDeep) };
  }
  m = /^not\((.*)\)$/.exec(t);
  if (m) return { kind: "not", members: [parseCondDeep(m[1])] };
  m = /^(eq|ne|lt|le|gt|ge)\((.*)\)$/.exec(t);
  if (m) {
    const parts = splitTopLevel(m[2], ",");
    if (parts.length === 2) return { kind: "cmp", op: m[1], left: parts[0], right: parts[1] };
  }
  m = /^in\((.*)\)$/.exec(t);
  if (m) {
    const parts = splitTopLevel(m[1], ",");
    return { kind: "in", value: parts[0] || "", values: parts.slice(1).join(", ") };
  }
  m = /^match\((.*)\)$/.exec(t);
  if (m) {
    const parts = splitTopLevel(m[1], ",");
    if (parts.length === 2) {
      return { kind: "match", value: parts[0], pattern: stripQuotes(parts[1]) };
    }
  }
  return { kind: "custom", text: t };
}

function condTextDeep(c) {
  if (!c) return "";
  if (c.kind === "cmp") return `${c.op}(${c.left}, ${c.right})`;
  if (c.kind === "in") return `in(${c.value}${c.values ? ", " + c.values : ""})`;
  if (c.kind === "match") {
    return `match(${c.value}, "${(c.pattern || "").replace(/"/g, '\\"')}")`;
  }
  if (c.kind === "and" || c.kind === "or") {
    return `${c.kind}(${(c.members || []).map(condTextDeep).join(", ")})`;
  }
  if (c.kind === "not") {
    return `not(${condTextDeep((c.members || [])[0] || { kind: "custom", text: "" })})`;
  }
  return c.text || "";
}

// ── 草稿树：group / validator / cond ───────────────────────
// conns[i] 是 children[i] 与下一个成员之间的连接符；最后一项的连接符不使用。

function connAt(conns, i) {
  return (conns && conns[i]) || "and";
}

function groupWith(children, conns) {
  return { type: "group", children, conns };
}

function emptyGroup() {
  return groupWith([], []);
}

function condLeaf(partial) {
  return {
    type: "cond",
    column: "",
    op: "eq",
    value: "",
    values: [],
    pattern: "",
    text: "",
    ...partial,
  };
}

function isConditionOnly(node) {
  if (!node) return false;
  if (node.type === "cond") return true;
  if (node.type === "group" && node.children.length > 0) {
    return node.children.every(isConditionOnly);
  }
  return false;
}

function condLeafText(l) {
  if (l.op === "custom") return (l.text || "").trim() || null;
  if (!(l.column || "").trim()) return null;
  if (l.op === "in") {
    const vals = (l.values || []).map((v) => String(v).trim()).filter((v) => v !== "");
    if (!vals.length) return null;
    return `in(${l.column}${vals.map((v) => `, ${v}`).join("")})`;
  }
  if (l.op === "match") {
    const pat = (l.pattern || "").trim();
    if (!pat) return null;
    return `match(${l.column}, "${pat.replace(/"/g, '\\"')}")`;
  }
  return `${l.op}(${l.column}, ${l.value})`;
}

/** 条件行按 AND 优先、OR 分段组合成一条条件表达式。 */
function combineConditions(items) {
  if (!items.length) return null;
  const terms = [];
  let cur = [];
  for (const [cond, conn] of items) {
    if (cond == null) return null;
    cur.push(cond);
    if (conn === "or") {
      terms.push(cur.length === 1 ? cur[0] : `and(${cur.join(", ")})`);
      cur = [];
    }
  }
  if (cur.length) terms.push(cur.length === 1 ? cur[0] : `and(${cur.join(", ")})`);
  if (!terms.length) return null;
  return terms.length === 1 ? terms[0] : `or(${terms.join(", ")})`;
}

function conditionOf(node) {
  if (node.type === "cond") return condLeafText(node);
  if (node.type !== "group") return null;
  const items = [];
  node.children.forEach((child, i) => items.push([conditionOf(child), connAt(node.conns, i)]));
  return combineConditions(items);
}

/** 验证器成员按同样的 AND 优先规则组合成规则表达式。 */
function combineRules(items) {
  if (!items.length) return null;
  const terms = [];
  let cur = [];
  const andOf = (list) =>
    list.length === 1 ? list[0] : { kind: "Group", op: "and", members: list };
  for (const [expr, conn] of items) {
    cur.push(expr);
    if (conn === "or") {
      terms.push(andOf(cur));
      cur = [];
    }
  }
  if (cur.length) terms.push(andOf(cur));
  if (!terms.length) return null;
  return terms.length === 1 ? terms[0] : { kind: "Group", op: "or", members: terms };
}

function exprFromDraft(node) {
  if (!node) return null;
  if (node.type === "validator") {
    return { kind: "Fun", name: node.name, args: [...(node.args || [])] };
  }
  if (node.type === "cond") return null;
  const conds = [];
  const rules = [];
  node.children.forEach((child, i) => {
    const conn = connAt(node.conns, i);
    if (isConditionOnly(child)) {
      conds.push([conditionOf(child), conn]);
    } else {
      const e = exprFromDraft(child);
      if (e) rules.push([e, conn]);
    }
  });
  const inner = combineRules(rules);
  if (!inner) return null;
  if (!conds.length) return inner;
  const cond = combineConditions(conds);
  if (!cond) return null;
  return { kind: "When", cond, inner };
}

function condNodeFromModel(m) {
  if (!m) return condLeaf({ op: "custom", text: "" });
  if (m.kind === "cmp") return condLeaf({ column: m.left, op: m.op, value: m.right });
  if (m.kind === "in") {
    const values = splitTopLevel(m.values || "", ",")
      .map(stripQuotes)
      .filter((v) => v !== "");
    return condLeaf({ column: m.value, op: "in", values });
  }
  if (m.kind === "match") return condLeaf({ column: m.value, op: "match", pattern: m.pattern });
  if (m.kind === "and" || m.kind === "or") {
    const children = (m.members || []).map(condNodeFromModel);
    return groupWith(children, children.map(() => m.kind));
  }
  // not(...) 暂时按「自定义条件」保留原文，图形界面不拆解。
  return condLeaf({ op: "custom", text: condTextDeep(m) });
}

function condDraftFromText(text) {
  return condNodeFromModel(parseCondDeep(text || ""));
}

function mergeWhen(cond, inner) {
  const [cc, ck] = cond.type === "group" ? [cond.children, [...cond.conns]] : [[cond], ["and"]];
  const [ic, ik] = inner.type === "group" ? [inner.children, [...inner.conns]] : [[inner], ["and"]];
  if (!cc.length) return groupWith(ic, ik);
  if (!ic.length) return groupWith(cc, ck);
  if (ck.length) ck[ck.length - 1] = "and";
  return groupWith([...cc, ...ic], [...ck, ...ik]);
}

function draftFromExpr(expr) {
  if (!expr) return emptyGroup();
  if (expr.kind === "Fun") {
    return { type: "validator", name: expr.name, args: [...(expr.args || [])] };
  }
  if (expr.kind === "When") {
    return mergeWhen(condDraftFromText(expr.cond), draftFromExpr(expr.inner));
  }
  const children = (expr.members || []).map(draftFromExpr);
  return groupWith(children, children.map(() => expr.op || "and"));
}

function normalizeNode(node) {
  if (node.type !== "group") return node;
  const children = node.children.map(normalizeNode);
  if (children.length === 1) return children[0];
  return groupWith(
    children,
    children.map((_, i) => connAt(node.conns, i)),
  );
}

/** 根必须是组；单成员组折叠成成员本身（与 Slint 版 normalize 一致）。 */
function normalizeDraft(draft) {
  if (draft.type !== "group") return groupWith([draft], ["and"]);
  const children = draft.children.map(normalizeNode);
  return groupWith(
    children,
    children.map((_, i) => connAt(draft.conns, i)),
  );
}

function pruneDraft(node) {
  if (node.type !== "group") return node;
  const children = [];
  const conns = [];
  node.children.forEach((child, i) => {
    const p = pruneDraft(child);
    if (p.type === "group" && p.children.length === 0) return;
    children.push(p);
    conns.push(connAt(node.conns, i));
  });
  node.children = children;
  node.conns = conns;
  return node;
}

// ── 路径 / 编辑操作 ────────────────────────────────────────

function draftAt(ruleIdx, path) {
  const rd = rulesState.drafts[ruleIdx];
  if (!rd) return null;
  let node = rd.draft;
  for (const seg of String(path || "").split(".").filter((s) => s !== "")) {
    if (!node || node.type !== "group") return null;
    node = node.children[Number(seg)];
  }
  return node || null;
}

function pathOf(parentPath, index) {
  return parentPath ? `${parentPath}.${index}` : `${index}`;
}

function defaultValidator() {
  const fns = rulesState.payload.functions.filter((f) => f.scopes.includes(rulesState.scope));
  const pick =
    (rulesState.scope === "table" && fns.find((f) => f.name === "row_count")) ||
    fns.find((f) => f.name === "non_empty") ||
    fns[0];
  return {
    type: "validator",
    name: pick?.name || "non_empty",
    args: pick ? defaultArgs(pick) : [],
  };
}

function insertNode(ruleIdx, parentPath, index, node) {
  const parent = draftAt(ruleIdx, parentPath);
  if (!parent || parent.type !== "group") return;
  const at = index == null || index < 0 ? 0 : Math.min(index + 1, parent.children.length);
  parent.children.splice(at, 0, node);
  parent.conns.splice(at, 0, "and");
  if (at > 0) parent.conns[at - 1] = "and";
  rulesState.dirty = true;
  rulesState.selected = { rule: ruleIdx, path: pathOf(parentPath, at) };
  renderRules();
}

function replaceAt(ruleIdx, path, next) {
  const segs = String(path || "")
    .split(".")
    .filter((s) => s !== "")
    .map(Number);
  const idx = segs.pop();
  const parent = draftAt(ruleIdx, segs.join("."));
  if (!parent || parent.type !== "group" || idx == null) return;
  parent.children[idx] = next;
}

function deleteDraftNode(ruleIdx, parentPath, index) {
  const parent = draftAt(ruleIdx, parentPath);
  if (!parent || parent.type !== "group") return;
  parent.children.splice(index, 1);
  parent.conns.splice(index, 1);
  pruneDraft(rulesState.drafts[ruleIdx].draft);
  rulesState.dirty = true;
  rulesState.selected = null;
  renderRules();
}

function dissolveNode(ruleIdx, parentPath, index) {
  const parent = draftAt(ruleIdx, parentPath);
  if (!parent || parent.type !== "group") return;
  const group = parent.children[index];
  if (!group || group.type !== "group") return;
  const external = connAt(parent.conns, index);
  parent.children.splice(index, 1);
  parent.conns.splice(index, 1);
  if (group.children.length) {
    group.conns[group.conns.length - 1] = external;
    parent.children.splice(index, 0, ...group.children);
    parent.conns.splice(index, 0, ...group.conns);
  }
  pruneDraft(rulesState.drafts[ruleIdx].draft);
  rulesState.dirty = true;
  rulesState.selected = null;
  renderRules();
}

function mergeRoot(ruleIdx) {
  const rd = rulesState.drafts[ruleIdx];
  const root = rd.draft;
  if (root.type !== "group" || root.children.length < 2) return;
  rd.draft = groupWith([groupWith([...root.children], [...root.conns])], ["and"]);
  rulesState.dirty = true;
  rulesState.selected = null;
  renderRules();
}

function setConnector(ruleIdx, parentPath, index, op) {
  const parent = draftAt(ruleIdx, parentPath);
  if (!parent || parent.type !== "group") return;
  parent.conns[index] = op === "or" ? "or" : "and";
  rulesState.dirty = true;
  renderRules();
}

// ── 文案 ──────────────────────────────────────────────────

function ruleSummaryText(rd) {
  const expr = exprFromDraft(rd.draft);
  if (!expr) return "（规则不完整）";
  return `${rd.target ? `column ${rd.target}` : "table"}: ${exprText(expr)}`;
}

function ruleExprText(rd) {
  const expr = exprFromDraft(rd.draft);
  return expr ? exprText(expr) : "（规则不完整）";
}

function validatorRowLabel(node) {
  return `${node.name}(${(node.args || []).join(", ")})`;
}

function condRowLabel(leaf) {
  if (leaf.op === "custom") return (leaf.text || "").trim() || "自定义条件";
  if (leaf.op === "in") return `${leaf.column} in (${(leaf.values || []).join(", ")})`;
  if (leaf.op === "match") return `${leaf.column} match "${leaf.pattern || ""}"`;
  const opLabel = (LEAF_OPS.find((o) => o[0] === leaf.op) || [leaf.op, leaf.op])[1];
  return `${leaf.column} ${opLabel} ${leaf.value ?? ""}`;
}

// ── 渲染：规则卡片 / 树行 ─────────────────────────────────

/** 规则行的函数下拉：验证器函数 + 条件函数（mockup 里「行」就是函数）。 */
function functionOptions(node) {
  const isCond = node?.type === "cond";
  const current = isCond ? `c:${node.op}` : `v:${node?.name || ""}`;
  const validators = rulesState.payload.functions.filter((f) => f.scopes.includes(rulesState.scope));
  if (!isCond && node && !validators.some((f) => f.name === node.name)) {
    const fn = fnByName(node.name);
    if (fn) validators.push(fn);
  }
  let html = validators
    .map(
      (f) =>
        `<option value="v:${escapeHtml(f.name)}"${
          current === `v:${f.name}` ? " selected" : ""
        } title="${escapeHtml(f.summary)}">${escapeHtml(f.name)}</option>`,
    )
    .join("");
  html += LEAF_OPS.map(
    ([op, label]) =>
      `<option value="c:${op}"${current === `c:${op}` ? " selected" : ""}>条件：${escapeHtml(
        label,
      )} (${op})</option>`,
  ).join("");
  return html;
}

function connSelect(ruleIdx, parentPath, index, conn) {
  return `<select class="conn" data-act="conn" data-rule="${ruleIdx}" data-parent="${escapeHtml(
    parentPath || "",
  )}" data-index="${index}" title="与下一项的连接符">` +
    `<option value="and"${conn === "and" ? " selected" : ""}>and</option>` +
    `<option value="or"${conn === "or" ? " selected" : ""}>or</option></select>`;
}

function rowActions(ruleIdx, parentPath, index, extra = "") {
  const p = escapeHtml(parentPath || "");
  const b = (act, text, cls = "", title = "") =>
    `<button class="mini-btn ${cls}" data-act="${act}" data-rule="${ruleIdx}" data-parent="${p}" data-index="${index}"${
      title ? ` title="${escapeHtml(title)}"` : ""
    }>${text}</button>`;
  return `<span class="ractions">${b("add-validator", "＋行", "", "在下方插入函数行")}${b(
    "add-group",
    "＋组",
    "",
    "在下方插入规则组",
  )}${extra}${b("del-node", "删除", "danger")}</span>`;
}

function renderRow(node, ruleIdx, path, depth, conn, showConn, parentPath, index) {
  const isCond = node.type === "cond";
  const selected =
    rulesState.selected && rulesState.selected.rule === ruleIdx && rulesState.selected.path === path;
  const label = isCond ? condRowLabel(node) : validatorRowLabel(node);
  return `<div class="rrow ${isCond ? "cond" : "validator"}${selected ? " selected" : ""}" data-rule="${ruleIdx}" data-path="${path}" data-selectable="1" style="margin-left:${
    depth * 14
  }px">
    <span class="rkind${isCond ? " cond" : ""}">${isCond ? "条件" : "验证器"}</span>
    <span class="row-fn" title="${escapeHtml(label)} —— 点击这一行编辑参数">${escapeHtml(label)}</span>
    ${showConn ? connSelect(ruleIdx, parentPath, index, conn) : ""}
    <span class="rspacer"></span>
    ${rowActions(ruleIdx, parentPath, index)}
  </div>`;
}

function renderEmptyLine(ruleIdx, parentPath) {
  return `<div class="rrow empty" style="margin-left:0">
    <span class="hint">空组：先添加一个函数行</span>
    <span class="rspacer"></span>
    <span class="ractions">
      <button class="mini-btn" data-act="add-validator" data-rule="${ruleIdx}" data-parent="${escapeHtml(
        parentPath || "",
      )}" data-index="-1">＋行</button>
      <button class="mini-btn" data-act="add-group" data-rule="${ruleIdx}" data-parent="${escapeHtml(
        parentPath || "",
      )}" data-index="-1">＋组</button>
    </span>
  </div>`;
}

function renderNode(node, ruleIdx, path, depth, conn, showConn, parentPath, index) {
  if (node.type !== "group") return renderRow(node, ruleIdx, path, depth, conn, showConn, parentPath, index);
  const inner = depth + 1;
  const body = node.children.length
    ? node.children
        .map((child, i) =>
          renderNode(
            child,
            ruleIdx,
            pathOf(path, i),
            inner,
            connAt(node.conns, i),
            i < node.children.length - 1,
            path,
            i,
          ),
        )
        .join("")
    : renderEmptyLine(ruleIdx, path);
  return `<div class="rgroup" data-path="${path}">
    <div class="rrow group-open" style="margin-left:${depth * 14}px">
      <span class="paren">(</span><span class="rtag" title="and / or 组">组</span>
    </div>
    ${body}
    <div class="rrow group-close" style="margin-left:${depth * 14}px">
      <span class="paren">)</span>
      ${showConn ? connSelect(ruleIdx, parentPath, index, conn) : ""}
      <span class="rspacer"></span>
      ${rowActions(ruleIdx, parentPath, index, `<button class="mini-btn" data-act="dissolve" data-rule="${ruleIdx}" data-parent="${escapeHtml(
        parentPath || "",
      )}" data-index="${index}" title="取消这个组，把成员移到上一层">分散</button>`)}
    </div>
  </div>`;
}

function renderRoot(ruleIdx) {
  const rd = rulesState.drafts[ruleIdx];
  const root = rd.draft;
  const children = root.children || [];
  const merge = `<div class="rroot-head"><span class="root-label">根组（隐式，不画外层括号）</span><span class="rspacer"></span>
           <button class="mini-btn" data-act="merge" data-rule="${ruleIdx}"${
             children.length < 2 ? " disabled" : ""
           } title="把根层所有成员包进一个组">合并</button></div>`;
  const body = children.length
    ? children
        .map((child, i) =>
          renderNode(
            child,
            ruleIdx,
            pathOf("", i),
            0,
            connAt(root.conns, i),
            i < children.length - 1,
            "",
            i,
          ),
        )
        .join("")
    : renderEmptyLine(ruleIdx, "");
  return `<div class="rroot">${merge}${body}</div>`;
}

function renderRuleCard(rd, idx) {
  const exprLine = ruleExprText(rd);
  const summary = ruleSummaryText(rd);
  const active = rulesState.selected?.rule === idx;
  const body = rd.collapsed
    ? ""
    : `<div class="rule-card-body">
        ${renderRoot(idx)}
        <div class="expr"><span class="lab">表达式</span><span class="expr-text">${escapeHtml(
          exprLine,
        )}</span></div>
      </div>`;
  return `<section class="rule-card${active ? " active" : ""}" data-rule-card="${idx}">
    <div class="rule-card-head">
      <b>规则 ${idx + 1}</b>
      <select class="level" data-act="level" data-rule="${idx}" title="规则级别">
        ${[
          ["error", "错误"],
          ["warn", "警告"],
          ["info", "信息"],
        ]
          .map(
            ([v, label]) =>
              `<option value="${v}"${rd.level === v ? " selected" : ""}>${label}</option>`,
          )
          .join("")}
      </select>
      <span class="rule-summary" title="${escapeHtml(summary)}">${escapeHtml(summary)}</span>
      <span class="rspacer"></span>
      <button class="mini-btn" data-act="toggle-rule" data-rule="${idx}">${
        rd.collapsed ? "展开" : "收起"
      }</button>
      <button class="mini-btn danger" data-act="del-rule" data-rule="${idx}">删除</button>
    </div>${body}</section>`;
}

function renderTargets() {
  const p = rulesState.payload;
  const counts = {};
  for (const rd of rulesState.drafts) counts[rd.target || ""] = (counts[rd.target || ""] || 0) + 1;
  if (rulesState.scope === "table") {
    rulesState.target = "";
    $("rules-targets").innerHTML =
      `<div class="panel-head">范围</div>` +
      `<div class="rule-target active" data-target="">表级规则</div>`;
    return;
  }
  $("rules-targets").innerHTML =
    `<div class="panel-head">列</div>` +
    p.columns
      .map(
        (c) =>
          `<div class="rule-target${c === rulesState.target ? " active" : ""}" data-target="${escapeHtml(
            c,
          )}"><span>${escapeHtml(c)}</span><span class="count">${
            counts[c] ? `${counts[c]} 条` : ""
          }</span></div>`,
      )
      .join("");
}

function renderCustom() {
  const t = currentTarget();
  const cards = [];
  rulesState.drafts.forEach((rd, idx) => {
    if ((rd.target || "") === t) cards.push(renderRuleCard(rd, idx));
  });
  $("rules-custom").innerHTML =
    cards.join("") ||
    `<div class="rules-empty">该范围还没有自定义规则，点下面「+ 新建规则」开始。</div>`;
}

function renderSystem() {
  const t = currentTarget();
  const rows = rulesState.payload.system.filter((r) => (r.target || "") === t);
  $("rules-system").innerHTML =
    `<div class="system-note">系统内置规则由引擎生成，始终与自定义规则 AND 生效；这里只读，不能编辑或删除。</div>` +
    (rows
      .map(
        (r) =>
          `<div class="sys-row"><span class="sys-tag">系统</span><span class="sys-text">${escapeHtml(
            r.text,
          )}</span><span class="rspacer"></span><span class="sys-level">${escapeHtml(
            r.level,
          )}</span><span class="sys-ro">只读</span></div>`,
      )
      .join("") || `<div class="sys-empty">该目标没有系统规则</div>`);
}

function renderStatus() {
  const t = currentTarget();
  const count = rulesState.drafts.filter((rd) => (rd.target || "") === t).length;
  const sys = rulesState.payload.system.filter((r) => (r.target || "") === t).length;
  const scopeText = rulesState.scope === "table" ? "表级规则" : `列规则 · ${t || "-"}`;
  $("rules-status").innerHTML =
    `<span>${escapeHtml(scopeText)}</span><b>${count} 条自定义规则</b><span>${sys} 条系统规则（只读）</span>` +
    (rulesState.dirty ? `<span class="dirty">未保存修改</span>` : "");
}

// ── 渲染：底部 inspector ──────────────────────────────────

function paramField(kind, index, value, ruleIdx, path) {
  const label = kind === "bound" ? (index === 0 ? "下限" : "上限") : PARAM_LABEL[kind] || kind;
  const attr = `data-act="dparam" data-rule="${ruleIdx}" data-path="${path}" data-idx="${index}"`;
  let input;
  if (kind === "compare_op") {
    input = `<select ${attr}>${["<", "<=", ">", ">=", "==", "!="]
      .map((v) => `<option${value === v ? " selected" : ""}>${v}</option>`)
      .join("")}</select>`;
  } else if (kind === "column") {
    const cols = rulesState.payload.columns || [];
    const extra = value && !cols.includes(value) ? [value] : [];
    input = `<select ${attr}>${[...extra, ...cols]
      .map((c) => `<option value="${escapeHtml(c)}"${c === value ? " selected" : ""}>${escapeHtml(c)}</option>`)
      .join("")}</select>`;
  } else {
    input = `<input ${attr} value="${escapeHtml(value)}" placeholder="${escapeHtml(
      PARAM_PLACEHOLDER[kind] || "",
    )}" spellcheck="false" />`;
  }
  return `<div class="field"><label>${escapeHtml(label)}</label>${input}</div>`;
}

function docsBlock(desc, use, example) {
  return `<h4>说明</h4><p>${escapeHtml(desc)}</p>
    <h4>适用</h4><p>${escapeHtml(use)}</p>
    <h4>示例</h4><div class="example">${escapeHtml(example)}</div>`;
}

function detailShell(title, tag, tagCls, right, paramsHtml, docsHtml) {
  return `<div class="detail-head">
      <b>${escapeHtml(title)}</b><span class="scope${tagCls ? ` ${tagCls}` : ""}">${escapeHtml(tag)}</span>
      <span class="rspacer"></span>${right || ""}
    </div>
    <div class="detail-body">
      <div class="detail-params">${paramsHtml}</div>
      <div class="detail-docs">${docsHtml}</div>
    </div>`;
}

function renderFunDetail(node, ruleIdx, path) {
  const fn = fnByName(node.name);
  const visible = visibleParams(fn);
  const fields = visible
    .map(({ kind, index }) => paramField(kind, index, node.args[index] ?? "", ruleIdx, path))
    .join("");
  const variadic =
    fn && fn.variadic
      ? `<div class="field"><label>${escapeHtml(PARAM_LABEL[fn.variadic] || fn.variadic)}</label>
         <input data-act="dvar" data-rule="${ruleIdx}" data-path="${path}" value="${escapeHtml(
           (node.args || []).slice(visible.length).join(", "),
         )}" placeholder="逗号分隔" spellcheck="false" /></div>`
      : "";
  const paramsHtml =
    `<h4>参数</h4>` +
    (fields || variadic
      ? fields + variadic
      : `<div class="hint">该函数没有参数。</div>`) +
    (rulesState.scope === "column"
      ? `<div class="hint">列级作用域下主列就是左侧选中的列，不需要重复填。</div>`
      : `<div class="hint">表级作用域下所有列参数都要显式填写。</div>`);
  const fnName = fn?.name || node.name;
  const exprNow = validatorRowLabel(node);
  const use = `${rulesState.scope === "column" ? "列级" : "表级"} · ${
    fn?.granularity === "table" ? "全表" : "逐行"
  }`;
  const example = `# @rule ${currentTarget() ? `column ${currentTarget()}` : "table"}: ${exprNow}`;
  const docsHtml = docsBlock(fn?.summary || "（未知函数）", use, example);
  const right = `<label class="detail-label">更换函数</label>
    <select class="detail-fn" data-act="dfn" data-rule="${ruleIdx}" data-path="${path}" title="${escapeHtml(
      fn?.summary || "",
    )}">${functionOptions(node)}</select>`;
  return detailShell(`${fnName}()`, "验证器", "", right, paramsHtml, docsHtml);
}

function condValueField(leaf, ruleIdx, path) {
  const attr = (act) => `data-act="${act}" data-rule="${ruleIdx}" data-path="${path}"`;
  if (leaf.op === "custom") {
    return `<div class="field"><label>表达式</label><input ${attr(
      "dcond-custom",
    )} value="${escapeHtml(leaf.text || "")}" placeholder="eq(kind, 2)" spellcheck="false" /></div>`;
  }
  if (leaf.op === "in") {
    return `<div class="field"><label>候选值</label><input ${attr(
      "dcond-values",
    )} value="${escapeHtml((leaf.values || []).join(", "))}" placeholder="1, 2, 3" spellcheck="false" /></div>`;
  }
  if (leaf.op === "match") {
    return `<div class="field"><label>正则</label><input ${attr(
      "dcond-pattern",
    )} value="${escapeHtml(leaf.pattern || "")}" placeholder="^...$" spellcheck="false" /></div>`;
  }
  return `<div class="field"><label>比较值</label><input ${attr(
    "dcond-value",
  )} value="${escapeHtml(leaf.value || "")}" placeholder="1" spellcheck="false" /></div>`;
}

function renderCondDetail(node, ruleIdx, path) {
  const cols = rulesState.payload.columns || [];
  const extra = node.column && !cols.includes(node.column) ? [node.column] : [];
  const colSelect = `<select data-act="dcond-column" data-rule="${ruleIdx}" data-path="${path}">${[
    ...extra,
    ...cols,
  ]
    .map((c) => `<option value="${escapeHtml(c)}"${c === node.column ? " selected" : ""}>${escapeHtml(c)}</option>`)
    .join("")}</select>`;
  const opSelect = `<select data-act="dcond-op" data-rule="${ruleIdx}" data-path="${path}">${LEAF_OPS.map(
    ([v, l]) => `<option value="${v}"${node.op === v ? " selected" : ""}>${l}</option>`,
  ).join("")}</select>`;
  const paramsHtml = `<h4>条件</h4>
    <div class="field"><label>列</label>${colSelect}</div>
    <div class="field"><label>运算</label>${opSelect}</div>
    ${condValueField(node, ruleIdx, path)}
    <div class="hint">条件行只过滤参与同组验证器的行；单独的条件不能保存成规则，要配一个验证器。</div>`;
  const example = `when(${condLeafText(node) || "<条件>"}, non_empty(${currentTarget() || "<列>"}))`;
  const docsHtml = docsBlock(
    "行过滤条件：条件不成立的行不参与同组验证器；全表函数也只统计命中的行。",
    "AND 优先、OR 分段；同组条件与验证器一起编译成 when(条件, 验证器)。",
    example,
  );
  const right = `<label class="detail-label">更换函数</label>
    <select class="detail-fn" data-act="dfn" data-rule="${ruleIdx}" data-path="${path}">${functionOptions(
      node,
    )}</select>`;
  return detailShell(
    node.op === "custom" ? "条件" : `${node.op}()`,
    "条件",
    "cond",
    right,
    paramsHtml,
    docsHtml,
  );
}

function renderDetail() {
  const host = $("rule-detail");
  const sel = rulesState.selected;
  const node = sel ? draftAt(sel.rule, sel.path) : null;
  if (!sel || !node || node.type === "group") {
    if (sel && (!node || node.type === "group")) rulesState.selected = null;
    host.innerHTML = `<div class="detail-empty">选中一条条件 / 验证器后，在这里编辑参数和查看说明。</div>`;
    return;
  }
  host.innerHTML =
    node.type === "cond"
      ? renderCondDetail(node, sel.rule, sel.path)
      : renderFunDetail(node, sel.rule, sel.path);
}

// ── 选择 / 汇总刷新 ───────────────────────────────────────

function setSelection(ruleIdx, path) {
  rulesState.selected = ruleIdx == null ? null : { rule: ruleIdx, path };
  document.querySelectorAll("#rules .rrow.selected").forEach((el) => el.classList.remove("selected"));
  document.querySelectorAll("#rules .rule-card.active").forEach((el) => el.classList.remove("active"));
  if (rulesState.selected) {
    const row = document.querySelector(
      `#rules .rrow[data-rule="${ruleIdx}"][data-path="${path}"]`,
    );
    if (row) row.classList.add("selected");
    const card = document.querySelector(`#rules .rule-card[data-rule-card="${ruleIdx}"]`);
    if (card) card.classList.add("active");
  }
  renderDetail();
}

/** 输入参数时只刷新摘要与行文案，避免整棵树重绘导致输入框丢焦点。 */
function refreshRuleText(ruleIdx) {
  const rd = rulesState.drafts[ruleIdx];
  if (!rd) return;
  const card = document.querySelector(`.rule-card[data-rule-card="${ruleIdx}"]`);
  if (card) {
    const sum = ruleSummaryText(rd);
    const sumEl = card.querySelector(".rule-summary");
    if (sumEl) {
      sumEl.textContent = sum;
      sumEl.title = sum;
    }
    const exprEl = card.querySelector(".expr-text");
    if (exprEl) exprEl.textContent = ruleExprText(rd);
  }
  const sel = rulesState.selected;
  if (sel && sel.rule === ruleIdx) {
    const node = draftAt(ruleIdx, sel.path);
    const row = document.querySelector(`#rules .rrow[data-rule="${ruleIdx}"][data-path="${sel.path}"] .row-fn`);
    if (row && node && node.type !== "group") {
      row.textContent = node.type === "cond" ? condRowLabel(node) : validatorRowLabel(node);
    }
  }
}

// ── 渲染入口 ──────────────────────────────────────────────

function renderRules() {
  const p = rulesState.payload;
  if (!p) return;
  $("rules-title").textContent = `校验规则 — ${p.group} / ${p.name}${
    p.mode === "table" ? " (Table)" : p.mode === "enum" ? " (Enum)" : ""
  }`;
  document.querySelectorAll("#rules-scope .scope-tab").forEach((b) => {
    b.classList.toggle("active", b.dataset.scope === rulesState.scope);
    if (b.dataset.scope === "table") b.hidden = !p.supports_table;
  });
  document.querySelectorAll(".rtab").forEach((b) =>
    b.classList.toggle("active", b.dataset.rtab === rulesState.tab),
  );
  $("rules-custom-pane").hidden = rulesState.tab !== "custom";
  $("rules-system").hidden = rulesState.tab !== "system";
  renderTargets();
  renderCustom();
  renderSystem();
  renderStatus();
  renderDetail();
}

// ── 打开 / 关闭 / 保存 ────────────────────────────────────

async function openRulesEditor() {
  const sel = state.selected;
  if (!sel || !sel.name) return addLocalLog(1, "请先选择一个节点");
  if (sel.kind === "constant") return addLocalLog(1, "Constant 不支持校验规则");
  try {
    const payload = await invoke("rules_payload", {
      project: sel.project,
      group: sel.group,
      name: sel.name,
    });
    rulesState.payload = payload;
    rulesState.scope = !payload.columns.length && payload.supports_table ? "table" : "column";
    rulesState.target = payload.columns[0] || "";
    rulesState.tab = "custom";
    rulesState.dirty = false;
    rulesState.selected = null;
    rulesState.drafts = payload.rules.map((r) => ({
      target: r.target || "",
      level: r.level,
      collapsed: false,
      draft: normalizeDraft(draftFromExpr(r.expr)),
    }));
    hideDiscardBar();
    $("rules-error").textContent = "";
    $("rules").hidden = false;
    renderRules();
  } catch (e) {
    addLocalLog(2, String(e));
  }
}

async function closeRulesEditor() {
  if (rulesState.dirty) {
    const ok = await openModal({
      title: "放弃修改",
      message: "校验规则有未保存的修改，确定关闭？",
      input: false,
    });
    if (!ok) return;
  }
  $("rules").hidden = true;
  hideDiscardBar();
  rulesState.payload = null;
  rulesState.selected = null;
  rulesState.drafts = [];
}

async function resetRules() {
  const p = rulesState.payload;
  if (!p) return;
  if (rulesState.dirty) {
    const ok = await openModal({
      title: "还原修改",
      message: "将丢弃未保存的修改，回到已保存的规则。确定还原？",
      input: false,
    });
    if (!ok) return;
  }
  try {
    const fresh = await invoke("rules_payload", { project: p.project, group: p.group, name: p.name });
    rulesState.payload = fresh;
    hideDiscardBar();
    rulesState.drafts = fresh.rules.map((r) => ({
      target: r.target || "",
      level: r.level,
      collapsed: false,
      draft: normalizeDraft(draftFromExpr(r.expr)),
    }));
    rulesState.dirty = false;
    rulesState.selected = null;
    if (rulesState.scope === "table" && !fresh.supports_table) rulesState.scope = "column";
    if (rulesState.scope === "column" && !fresh.columns.includes(rulesState.target)) {
      rulesState.target = fresh.columns[0] || "";
    }
    $("rules-error").textContent = "";
    renderRules();
  } catch (e) {
    addLocalLog(2, String(e));
  }
}

function addRule() {
  // 新卡片默认带一个验证器行（与 rule-builder-v2 / Slint 一致），并直接选中它。
  rulesState.drafts.push({
    target: currentTarget(),
    level: "error",
    collapsed: false,
    draft: groupWith([defaultValidator()], ["and"]),
  });
  const ruleIdx = rulesState.drafts.length - 1;
  rulesState.dirty = true;
  rulesState.selected = { rule: ruleIdx, path: "0" };
  renderRules();
  // 新规则滚动到可见位置。
  const card = document.querySelector(`.rule-card[data-rule-card="${ruleIdx}"]`);
  card?.scrollIntoView({ block: "nearest" });
}

async function saveRules() {
  const p = rulesState.payload;
  if (!p) return;
  hideDiscardBar();
  $("rules-error").textContent = "";
  const rules = [];
  for (let i = 0; i < rulesState.drafts.length; i++) {
    const rd = rulesState.drafts[i];
    pruneDraft(rd.draft);
    const root = rd.draft.type === "group" ? rd.draft : emptyGroup();
    if (!root.children.length) continue; // 完全空的规则直接丢弃
    const expr = exprFromDraft(root);
    if (!expr) {
      $("rules-error").textContent = `规则 ${i + 1} 不完整：至少需要一个验证器；条件行不能单独成规则。`;
      return;
    }
    rules.push({ target: rd.target || "", level: rd.level, expr });
  }
  try {
    const snap = await invoke("rules_save", {
      project: p.project,
      group: p.group,
      name: p.name,
      rules,
    });
    applySnapshot(snap);
    rulesState.dirty = false;
    $("rules").hidden = true;
    rulesState.payload = null;
    rulesState.selected = null;
    rulesState.drafts = [];
    addLocalLog(0, `已保存校验规则：${p.group}/${p.name}（${rules.length} 条自定义规则）`);
  } catch (e) {
    $("rules-error").textContent = String(e);
  }
}

// ── 事件 ─────────────────────────────────────────────────

/** 有未保存改动时切换作用域 / 列：先显示内联确认条（与 Slint 版一致）。 */
function requestSwitch(next) {
  if (rulesState.dirty) {
    const label = next.scope === "table" ? "「表级规则」" : `列「${next.target || ""}」`;
    $("rules-discard-msg").textContent = `切换到${label}会丢弃当前未保存的规则。`;
    $("rules-discard").hidden = false;
    rulesState.pendingSwitch = next;
    return;
  }
  applySwitch(next);
}

function hideDiscardBar() {
  rulesState.pendingSwitch = null;
  $("rules-discard").hidden = true;
}

function rebuildDraftsFromPayload() {
  rulesState.drafts = rulesState.payload.rules.map((r) => ({
    target: r.target || "",
    level: r.level,
    collapsed: false,
    draft: normalizeDraft(draftFromExpr(r.expr)),
  }));
  rulesState.dirty = false;
  rulesState.selected = null;
}

function applySwitch(next) {
  hideDiscardBar();
  rulesState.scope = next.scope || rulesState.scope;
  if (rulesState.scope === "table") {
    rulesState.target = "";
  } else if (next.target !== undefined) {
    rulesState.target = next.target;
  }
  rulesState.selected = null;
  renderRules();
}

function confirmDiscardSwitch() {
  const next = rulesState.pendingSwitch;
  if (!next) return;
  // 丢弃本次草稿，回到已保存 / 打开时的规则，再切到目标作用域。
  rebuildDraftsFromPayload();
  applySwitch(next);
}

function onRulesClick(e) {
  if (e.target.closest("#rules-discard-ok")) return confirmDiscardSwitch();
  if (e.target.closest("#rules-discard-cancel")) return hideDiscardBar();

  const scopeBtn = e.target.closest("[data-scope]");
  if (scopeBtn) {
    const want = scopeBtn.dataset.scope;
    if (want === rulesState.scope) return;
    requestSwitch({ scope: want, target: want === "table" ? "" : rulesState.target });
    return;
  }
  const targetBtn = e.target.closest(".rule-target[data-target]");
  if (targetBtn) {
    const want = targetBtn.dataset.target;
    if (rulesState.scope === "column" && want === rulesState.target) return;
    requestSwitch({ scope: "column", target: want });
    return;
  }
  const rtab = e.target.closest("[data-rtab]");
  if (rtab) {
    rulesState.tab = rtab.dataset.rtab;
    renderRules();
    return;
  }
  if (e.target.closest("#rules-reset")) return resetRules();
  if (e.target.closest("#rules-cancel")) return closeRulesEditor();
  if (e.target.closest("#rules-save")) return saveRules();
  if (e.target.closest("#rules-add")) return addRule();

  const actEl = e.target.closest("[data-act]");
  if (!actEl) {
    const row = e.target.closest(".rrow[data-selectable]");
    if (row) {
      const ruleIdx = Number(row.dataset.rule);
      const path = row.dataset.path;
      const sel = rulesState.selected;
      if (!sel || sel.rule !== ruleIdx || sel.path !== path) setSelection(ruleIdx, path);
    }
    return;
  }
  const act = actEl.dataset.act;
  const ruleIdx = actEl.dataset.rule !== undefined ? Number(actEl.dataset.rule) : -1;
  const parentPath = actEl.dataset.parent || "";
  const index = actEl.dataset.index !== undefined ? Number(actEl.dataset.index) : -1;

  switch (act) {
    case "toggle-rule":
      rulesState.drafts[ruleIdx].collapsed = !rulesState.drafts[ruleIdx].collapsed;
      renderRules();
      break;
    case "del-rule":
      rulesState.drafts.splice(ruleIdx, 1);
      rulesState.selected = null;
      rulesState.dirty = true;
      renderRules();
      break;
    case "merge":
      mergeRoot(ruleIdx);
      break;
    case "add-validator":
      insertNode(ruleIdx, parentPath, index, defaultValidator());
      break;
    case "add-group":
      insertNode(ruleIdx, parentPath, index, groupWith([defaultValidator()], ["and"]));
      break;
    case "del-node":
      deleteDraftNode(ruleIdx, parentPath, index);
      break;
    case "dissolve":
      dissolveNode(ruleIdx, parentPath, index);
      break;
    default:
      break;
  }
}

function onRulesInput(e) {
  const el = e.target;
  const act = el.dataset.act;
  if (!act) return;
  const ruleIdx = Number(el.dataset.rule);
  const path = el.dataset.path;
  const node = draftAt(ruleIdx, path);
  if (!node) return;
  if (act === "dparam") {
    if (node.type !== "validator") return;
    node.args[Number(el.dataset.idx)] = el.value;
  } else if (act === "dvar") {
    if (node.type !== "validator") return;
    const fn = fnByName(node.name);
    const fixed = visibleParams(fn).length;
    node.args = node.args
      .slice(0, fixed)
      .concat(el.value.split(",").map((s) => s.trim()).filter((s) => s !== ""));
  } else if (act === "dcond-value") {
    if (node.type !== "cond") return;
    node.value = el.value;
  } else if (act === "dcond-values") {
    if (node.type !== "cond") return;
    node.values = el.value.split(",").map((s) => s.trim()).filter((s) => s !== "");
  } else if (act === "dcond-pattern") {
    if (node.type !== "cond") return;
    node.pattern = el.value;
  } else if (act === "dcond-custom") {
    if (node.type !== "cond") return;
    node.text = el.value;
  } else {
    return;
  }
  rulesState.dirty = true;
  refreshRuleText(ruleIdx);
  renderStatus();
}

function onRulesChange(e) {
  const el = e.target;
  const act = el.dataset.act;
  if (!act) return;
  const ruleIdx = Number(el.dataset.rule);
  if (act === "level") {
    rulesState.drafts[ruleIdx].level = el.value;
    rulesState.dirty = true;
    renderStatus();
    return;
  }
  if (act === "conn") {
    setConnector(ruleIdx, el.dataset.parent || "", Number(el.dataset.index), el.value);
    return;
  }
  const path = el.dataset.path;
  const node = draftAt(ruleIdx, path);
  if (act === "dfn") {
    if (!node) return;
    const [tag, name] = el.value.split(":");
    if (tag === "v") {
      const next = { type: "validator", name, args: defaultArgs(fnByName(name)) };
      if (node.type === "validator") Object.assign(node, next);
      else replaceAt(ruleIdx, el.dataset.path, next);
    } else {
      const leaf = condLeaf({
        column: firstColumn(),
        op: name,
        value: ["in", "match", "custom"].includes(name) ? "" : "1",
        values: name === "in" ? ["1"] : [],
      });
      if (node.type === "cond") Object.assign(node, leaf);
      else replaceAt(ruleIdx, el.dataset.path, leaf);
    }
    rulesState.dirty = true;
    renderRules();
    return;
  }
  if (act === "dparam") {
    if (!node || node.type !== "validator") return;
    node.args[Number(el.dataset.idx)] = el.value;
    rulesState.dirty = true;
    refreshRuleText(ruleIdx);
    return;
  }
  if (act === "dcond-column") {
    if (!node || node.type !== "cond") return;
    node.column = el.value;
    rulesState.dirty = true;
    refreshRuleText(ruleIdx);
    return;
  }
  if (act === "dcond-op") {
    if (!node || node.type !== "cond") return;
    node.op = el.value;
    if (node.op === "in" && !(node.values || []).length) node.values = ["1"];
    if (node.op === "custom" && !(node.text || "").trim()) node.text = "";
    rulesState.dirty = true;
    renderRules();
  }
}

function initRulesEditor() {
  $("rules").addEventListener("click", onRulesClick);
  $("rules").addEventListener("input", onRulesInput);
  $("rules").addEventListener("change", onRulesChange);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("rules").hidden && $("modal").hidden) closeRulesEditor();
  });
}

// 跟随系统深浅色；也允许调试脚本写 data-theme 覆盖。
function syncSystemTheme() {
  const dark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

// ── 全局设置 / 导出 ────────────────────────────────────────
// ── 全局设置（UI / 分隔符模板 / 导出模板）─────────────────
// 参照物 = 打开对话框那一刻的 tablet.toml；每个改过的字段可单独 ↺，
// 「使用默认值」重置当前 tab，「取消」丢弃整个编辑缓冲。
let gsState = { original: null, defaults: null };

const GS_UI_FIELDS = {
  auto_commit_on_blur: "gs-auto-commit",
  realtime_validate: "gs-realtime",
  show_meta_id: "gs-show-id",
  constant_ref_allowed: "gs-const-ref",
  picker_trigger_header: "gs-header-trigger",
  picker_trigger_data: "gs-data-trigger",
  ref_picker_strategy: "gs-ref-strategy",
  log_level: "gs-log-level",
};
const GS_UI_DEFAULTS = {
  auto_commit_on_blur: true,
  realtime_validate: false,
  show_meta_id: false,
  constant_ref_allowed: true,
  picker_trigger_header: "single",
  picker_trigger_data: "double",
  ref_picker_strategy: "auto",
  log_level: "debug",
};
const gsId = (psId) => psId.replace(/^ps-/, "gs-");

function gsUiValue(key) {
  const el = $(GS_UI_FIELDS[key]);
  return el?.type === "checkbox" ? !!el.checked : el?.value ?? "";
}
function setGsUiValue(key, value) {
  const el = $(GS_UI_FIELDS[key]);
  if (!el) return;
  if (el.type === "checkbox") el.checked = !!value;
  else el.value = value;
}
function setGsExportFields(e) {
  for (const [key, psId] of psExportEntries()) {
    const el = $(gsId(psId));
    if (!el) continue;
    if (EXPORT_BOOL_FIELDS[key]) el.checked = !!e[key];
    else el.value = e[key] ?? "";
  }
}
function collectGsExport() {
  const out = {};
  for (const [key, psId] of Object.entries(EXPORT_TEXT_FIELDS)) out[key] = $(gsId(psId))?.value.trim() ?? "";
  for (const [key, psId] of Object.entries(EXPORT_BOOL_FIELDS)) out[key] = !!$(gsId(psId))?.checked;
  return out;
}

function setGsTab(tab) {
  document.querySelectorAll(".ps-tab[data-gstab]").forEach((t) => t.classList.toggle("active", t.dataset.gstab === tab));
  document.querySelectorAll("#global-settings .ps-pane").forEach((p) => {
    p.hidden = p.dataset.gspane !== tab;
  });
  refreshGsModified();
}

function renderGsSeparators(items) {
  state.gsSeparators = items;
  $("gs-seps").innerHTML = items
    .map(
      (it) =>
        `<div class="sep-row" data-gs-sep-row="${escapeHtml(it.key)}"><label title="${escapeHtml(
          it.key,
        )}">${escapeHtml(it.key)}</label>` +
        `<input data-gs-sep-key="${escapeHtml(it.key)}" value="${escapeHtml(it.value)}" placeholder="${escapeHtml(
          it.default,
        )}" /><button class="ps-rev" data-gs-sep-rev="${escapeHtml(
          it.key,
        )}" title="还原到打开时的值">↺</button></div>`,
    )
    .join("");
}

/** 给 UI / 导出字段补 ↺ 按钮（只补一次）。 */
function ensureGsRevertButtons() {
  for (const [key, id] of Object.entries(GS_UI_FIELDS)) {
    const el = $(id);
    if (!el || el.dataset.gsRevDone) continue;
    el.dataset.gsRevDone = "1";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ps-rev";
    btn.dataset.gsRev = key;
    btn.title = "还原到打开时的值";
    btn.textContent = "↺";
    const label = el.closest("label.check");
    if (label) label.insertAdjacentElement("afterend", btn);
    else (el.closest(".form-row") || el.parentElement).appendChild(btn);
  }
  for (const [, psId] of psExportEntries()) {
    const el = $(gsId(psId));
    if (!el || el.dataset.gsRevDone) continue;
    el.dataset.gsRevDone = "1";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ps-rev";
    btn.dataset.gsRevExport = gsId(psId);
    btn.title = "还原到打开时的值";
    btn.textContent = "↺";
    const label = el.closest("label.check");
    if (label) label.insertAdjacentElement("afterend", btn);
    else (el.closest(".form-row") || el.parentElement).appendChild(btn);
  }
}

function refreshGsModified() {
  const orig = gsState.original;
  if (!orig || $("global-settings").hidden) return;
  for (const key of Object.keys(GS_UI_FIELDS)) {
    const el = $(GS_UI_FIELDS[key]);
    const isBool = el?.type === "checkbox";
    const mod = isBool ? el.checked !== !!orig.ui[key] : el.value !== String(orig.ui[key] ?? "");
    markPs(el, mod);
    if (isBool) markPs(el.closest("label.check") || el, mod);
    const rev = document.querySelector(`[data-gs-rev="${key}"]`);
    if (rev) rev.disabled = !mod;
  }
  document.querySelectorAll("#gs-seps input[data-gs-sep-key]").forEach((input) => {
    const expected = orig.seps[input.dataset.gsSepKey] ?? "";
    const mod = input.value !== expected;
    markPs(input, mod);
    const rev = document.querySelector(`[data-gs-sep-rev="${CSS.escape(input.dataset.gsSepKey)}"]`);
    if (rev) rev.disabled = !mod;
  });
  for (const [key, psId] of psExportEntries()) {
    const el = $(gsId(psId));
    if (!el) continue;
    const isBool = !!EXPORT_BOOL_FIELDS[key];
    const mod = isBool
      ? el.checked !== !!orig.export[key]
      : el.value !== String(orig.export[key] ?? "");
    markPs(el, mod);
    if (isBool) markPs(el.closest("label.check") || el, mod);
    const rev = document.querySelector(`[data-gs-rev-export="${gsId(psId)}"]`);
    if (rev) rev.disabled = !mod;
  }
}

/** 使用默认值：只重置当前 tab（与 Slint 版一致）。 */
function applyGsDefaults() {
  const defs = gsState.defaults;
  if (!defs) return;
  const tab = document.querySelector(".ps-tab[data-gstab].active")?.dataset.gstab || "ui";
  if (tab === "ui") {
    for (const key of Object.keys(GS_UI_FIELDS)) setGsUiValue(key, defs.ui[key]);
  } else if (tab === "sep") {
    document.querySelectorAll("#gs-seps input[data-gs-sep-key]").forEach((input) => {
      input.value = defs.seps[input.dataset.gsSepKey] ?? "";
    });
  } else {
    setGsExportFields(defs.export);
  }
  refreshGsModified();
}

async function openGlobalSettings() {
  try {
    const s = await invoke("global_settings");
    setGsUiValue("auto_commit_on_blur", s.auto_commit_on_blur);
    setGsUiValue("realtime_validate", s.realtime_validate);
    setGsUiValue("show_meta_id", s.show_meta_id);
    setGsUiValue("constant_ref_allowed", s.constant_ref_allowed);
    setGsUiValue("picker_trigger_header", s.picker_trigger_header || "single");
    setGsUiValue("picker_trigger_data", s.picker_trigger_data || "double");
    setGsUiValue("ref_picker_strategy", s.ref_picker_strategy || "auto");
    setGsUiValue("log_level", s.log_level || "debug");
    renderGsSeparators(s.separators || []);
    $("global-settings").hidden = false; // refreshGsModified 需要可见
    setGsExportFields(s.export_settings || {});
    gsState = {
      original: {
        ui: {
          auto_commit_on_blur: !!s.auto_commit_on_blur,
          realtime_validate: !!s.realtime_validate,
          show_meta_id: !!s.show_meta_id,
          constant_ref_allowed: !!s.constant_ref_allowed,
          picker_trigger_header: s.picker_trigger_header || "single",
          picker_trigger_data: s.picker_trigger_data || "double",
          ref_picker_strategy: s.ref_picker_strategy || "auto",
          log_level: s.log_level || "debug",
        },
        seps: Object.fromEntries((s.separators || []).map((x) => [x.key, x.value ?? ""])),
        export: s.export_settings || {},
      },
      defaults: {
        ui: { ...GS_UI_DEFAULTS },
        seps: Object.fromEntries((s.separators || []).map((x) => [x.key, x.default ?? ""])),
        export: s.default_export || {},
      },
    };
    ensureGsRevertButtons();
    setGsTab("ui");
    $("gs-error").hidden = true;
    refreshGsModified();
  } catch (e) {
    addLocalLog(2, String(e));
  }
}

async function submitGlobalSettings() {
  const separators = [...document.querySelectorAll("#gs-seps input[data-gs-sep-key]")].map((i) => ({
    key: i.dataset.gsSepKey,
    value: i.value,
  }));
  try {
    const snap = await invoke("save_global_settings", {
      settings: {
        auto_commit_on_blur: $("gs-auto-commit").checked,
        realtime_validate: $("gs-realtime").checked,
        log_level: $("gs-log-level").value,
        picker_trigger_header: $("gs-header-trigger").value,
        picker_trigger_data: $("gs-data-trigger").value,
        show_meta_id: $("gs-show-id").checked,
        constant_ref_allowed: $("gs-const-ref").checked,
        ref_picker_strategy: $("gs-ref-strategy").value,
        separators,
        export_settings: collectGsExport(),
        default_export: gsState.defaults?.export || {},
      },
    });
    $("global-settings").hidden = true;
    applySnapshot(snap);
    addLocalLog(0, "全局设置已保存");
  } catch (e) {
    showFormError($("gs-error"), String(e));
  }
}

function initGlobalSettings() {
  // 导出模板页复用项目设置的 29 个字段：克隆静态标记并把 id 前缀换成 gs-。
  const exportSrc = document.querySelector('#project-settings .ps-pane[data-pane="export"]');
  $("gs-export").innerHTML = (exportSrc?.innerHTML || "").replace(/id="ps-/g, 'id="gs-');
  document.querySelectorAll(".ps-tab[data-gstab]").forEach((t) =>
    t.addEventListener("click", () => setGsTab(t.dataset.gstab)),
  );
  $("gs-defaults").addEventListener("click", applyGsDefaults);
  $("gs-cancel").addEventListener("click", () => ($("global-settings").hidden = true));
  $("gs-ok").addEventListener("click", submitGlobalSettings);
  $("btn-global-settings").addEventListener("click", openGlobalSettings);
  $("global-settings").addEventListener("input", refreshGsModified);
  $("global-settings").addEventListener("change", refreshGsModified);
  $("global-settings").addEventListener("click", (e) => {
    const uiRev = e.target.closest("[data-gs-rev]");
    if (uiRev && gsState.original) {
      setGsUiValue(uiRev.dataset.gsRev, gsState.original.ui[uiRev.dataset.gsRev]);
      refreshGsModified();
      return;
    }
    const sepRev = e.target.closest("[data-gs-sep-rev]");
    if (sepRev && gsState.original) {
      const input = document.querySelector(
        `#gs-seps input[data-gs-sep-key="${CSS.escape(sepRev.dataset.gsSepRev)}"]`,
      );
      if (input) input.value = gsState.original.seps[input.dataset.gsSepKey] ?? "";
      refreshGsModified();
      return;
    }
    const exRev = e.target.closest("[data-gs-rev-export]");
    if (exRev && gsState.original) {
      const id = exRev.dataset.gsRevExport;
      const key = psExportEntries().find(([, v]) => gsId(v) === id)?.[0];
      if (key) {
        const el = $(id);
        if (EXPORT_BOOL_FIELDS[key]) el.checked = !!gsState.original.export[key];
        else el.value = gsState.original.export[key] ?? "";
        refreshGsModified();
      }
    }
  });
}

// ── 导出执行 ────────────────────────────────────────────────
function openExportDialog() {
  $("ex-result").hidden = true;
  $("ex-result").textContent = "";
  $("ex-error").hidden = true;
  $("export-dialog").hidden = false;
}

async function runExport() {
  const formats = {};
  document.querySelectorAll("#ex-formats input[data-fmt]").forEach((i) => {
    formats[i.dataset.fmt] = i.checked;
  });
  $("ex-error").hidden = true;
  $("ex-result").hidden = false;
  $("ex-result").textContent = "导出中…";
  try {
    const report = await invoke("export_project", { formats });
    $("ex-result").textContent = report.lines.join("\n");
    if (!report.ok) showFormError($("ex-error"), "部分格式导出失败，详见上方结果");
    const snap = await invoke("snapshot");
    applySnapshot(snap);
    addLocalLog(report.ok ? 0 : 2, `导出完成：${report.lines.length} 项`);
    toast(report.ok ? "导出完成" : "部分格式导出失败", report.ok ? "ok" : "error");
  } catch (e) {
    $("ex-result").hidden = true;
    showFormError($("ex-error"), String(e));
  }
}

function initExportDialog() {
  $("btn-export").addEventListener("click", openExportDialog);
  $("ex-cancel").addEventListener("click", () => ($("export-dialog").hidden = true));
  $("ex-run").addEventListener("click", runExport);
  $("ex-all").addEventListener("click", () =>
    document.querySelectorAll("#ex-formats input[data-fmt]").forEach((i) => (i.checked = true)),
  );
  $("ex-none").addEventListener("click", () =>
    document.querySelectorAll("#ex-formats input[data-fmt]").forEach((i) => (i.checked = false)),
  );
  // 与 Slint 版一致的四个快捷预设。
  const presets = {
    all: ["json", "xml", "bin", "java", "go", "cpp", "csharp_dotnet", "typescript_server",
      "lua", "gdscript", "typescript_client", "csharp_unity", "csharp_godot"],
    data: ["json", "xml", "bin"],
    bin: ["bin"],
    server: ["json", "xml", "java", "go", "cpp", "csharp_dotnet", "typescript_server"],
    client: ["lua", "gdscript", "typescript_client", "csharp_unity", "csharp_godot"],
  };
  document.querySelectorAll("#export-dialog [data-expre]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const want = new Set(presets[btn.dataset.expre] || []);
      document.querySelectorAll("#ex-formats input[data-fmt]").forEach((i) => {
        i.checked = want.has(i.dataset.fmt);
      });
    });
  });
}

// ── Toast ───────────────────────────────────────────────────
function toast(message, level = "info") {
  const el = document.createElement("div");
  el.className = `toast ${level}`;
  el.textContent = message;
  $("toast-wrap").appendChild(el);
  setTimeout(() => el.remove(), 3000);
}

// ── Schema 导入 / 导出 ──────────────────────────────────────
// ── Schema 导入 / 导出（对齐 Slint 的节点树对话框）──────────
const schemaState = {
  mode: "export", // export | template | import
  outline: [],
  preview: null,
  selectedOut: new Set(),
  selectedIn: new Set(),
};

const SCHEMA_ICON = { table: "📊", constant: "📋", enum: "🔢" };

function schemaModeText() {
  if (schemaState.mode === "import") return { title: "导入 Schema", action: "导入" };
  if (schemaState.mode === "template") {
    return { title: "保存为本地模板", action: "保存到本地模板" };
  }
  return { title: "导出 Schema", action: "导出…" };
}

function updateSchemaChrome() {
  const m = schemaModeText();
  $("schema-title").textContent = m.title;
  $("schema-run").textContent = m.action;
  $("schema-id-label").textContent = schemaState.mode === "template" ? "模板 ID *" : "ID";
  $("schema-out-path-row").hidden = schemaState.mode === "template";
}

function schemaGroupsFromPreview() {
  const map = new Map();
  for (const s of schemaState.preview?.sections || []) {
    if (!map.has(s.group)) map.set(s.group, []);
    map.get(s.group).push({ name: s.name, kind: s.mode, exists: s.exists });
  }
  return [...map.entries()].map(([group, nodes]) => ({ group, nodes }));
}

function syncSchemaGroups(kind) {
  const out = kind === "out";
  const sel = out ? schemaState.selectedOut : schemaState.selectedIn;
  const groups = out ? schemaState.outline : schemaGroupsFromPreview();
  document.querySelectorAll(`#schema-${kind}-tree [data-sg]`).forEach((cb) => {
    const g = groups.find((x) => x.group === cb.dataset.sg);
    if (!g) return;
    const keys = g.nodes.map((n) => `${g.group}/${n.name}`);
    const on = keys.filter((k) => sel.has(k)).length;
    cb.checked = on === keys.length && on > 0;
    cb.indeterminate = on > 0 && on < keys.length;
  });
  const all = $(`schema-all-${kind}`);
  const total = groups.reduce((n, g) => n + g.nodes.length, 0);
  if (all) {
    all.checked = total > 0 && sel.size === total;
    all.indeterminate = sel.size > 0 && sel.size < total;
  }
}

function updateSchemaStats() {
  const total = schemaState.outline.reduce((n, g) => n + g.nodes.length, 0);
  $("schema-out-stat").textContent = `已选 ${schemaState.selectedOut.size}/${total} 项`;
  if (schemaState.preview) {
    const totalIn = schemaState.preview.sections.length;
    const conflicts = schemaState.preview.sections.filter(
      (s) => schemaState.selectedIn.has(`${s.group}/${s.name}`) && s.exists,
    ).length;
    $("schema-in-stat").textContent =
      `已选 ${schemaState.selectedIn.size}/${totalIn} 项` +
      (conflicts ? ` | 冲突: ${conflicts} 项将覆盖` : "");
  }
}

function renderSchemaOutTree() {
  const groups = schemaState.outline;
  $("schema-out-tree").innerHTML =
    groups
      .map((g) => {
        const keys = g.nodes.map((n) => `${g.group}/${n.name}`);
        const all = keys.every((k) => schemaState.selectedOut.has(k));
        return (
          `<div class="schema-group"><label class="check"><input type="checkbox" data-sg="${escapeHtml(
            g.group,
          )}" ${all ? "checked" : ""}/> ${escapeHtml(g.group)}</label></div>` +
          g.nodes
            .map((n) => {
              const key = `${g.group}/${n.name}`;
              return `<label class="schema-node"><input type="checkbox" data-sn="${escapeHtml(
                key,
              )}" ${schemaState.selectedOut.has(key) ? "checked" : ""}/><span>${
                SCHEMA_ICON[n.kind] || "📄"
              } ${escapeHtml(n.name)}</span><span class="meta">${n.kind}</span></label>`;
            })
            .join("")
        );
      })
      .join("") || `<div class="schema-empty">当前项目没有可导出的节点</div>`;
  syncSchemaGroups("out");
  updateSchemaStats();
}

function renderSchemaInTree() {
  if (!schemaState.preview) {
    $("schema-in-tree").innerHTML = "";
    $("schema-in-stat").textContent = "";
    return;
  }
  const groups = schemaGroupsFromPreview();
  $("schema-in-tree").innerHTML = groups
    .map((g) => {
      const keys = g.nodes.map((n) => `${g.group}/${n.name}`);
      const all = keys.every((k) => schemaState.selectedIn.has(k));
      return (
        `<div class="schema-group"><label class="check"><input type="checkbox" data-sg="${escapeHtml(
          g.group,
        )}" ${all ? "checked" : ""}/> ${escapeHtml(g.group)}</label></div>` +
        g.nodes
          .map((n) => {
            const key = `${g.group}/${n.name}`;
            const badge = n.exists
              ? `<span class="schema-conflict">⚠ 已存在(将覆盖)</span>`
              : `<span class="schema-new">✓ 新增</span>`;
            return `<label class="schema-node"><input type="checkbox" data-sn="${escapeHtml(
              key,
            )}" ${schemaState.selectedIn.has(key) ? "checked" : ""}/><span>${
              SCHEMA_ICON[n.kind] || "📄"
            } ${escapeHtml(n.name)}</span><span class="meta">${n.kind}</span>${badge}</label>`;
          })
          .join("")
      );
    })
    .join("");
  syncSchemaGroups("in");
  updateSchemaStats();
}

async function loadSchemaImportPreview() {
  const path = $("schema-in").value.trim();
  schemaState.preview = null;
  $("schema-in-body").hidden = true;
  $("schema-in-empty").hidden = false;
  $("schema-error").hidden = true;
  if (!path) return;
  try {
    const pv = await invoke("schema_import_preview", { path });
    schemaState.preview = pv;
    schemaState.selectedIn = new Set(pv.sections.map((s) => `${s.group}/${s.name}`));
    $("schema-in-body").hidden = false;
    $("schema-preset-in").checked = !!pv.has_preset;
    $("schema-preset-in-wrap").hidden = !pv.has_preset;
    renderSchemaInTree();
  } catch (e) {
    showFormError($("schema-error"), String(e));
  }
}

async function openSchemaDialog(mode = "export") {
  schemaState.mode = mode;
  schemaState.preview = null;
  $("schema-result").hidden = true;
  $("schema-error").hidden = true;
  const isImport = mode === "import";
  document.querySelectorAll("#schema-dialog .ps-pane").forEach((p) => {
    p.hidden = p.dataset.schem !== (isImport ? "import" : "export");
  });
  updateSchemaChrome();
  $("schema-dialog").hidden = false;
  if (isImport) {
    schemaState.selectedIn = new Set();
    $("schema-in-body").hidden = true;
    $("schema-in-empty").hidden = false;
    renderSchemaInTree();
    return;
  }
  // 每次打开都从空白元数据开始（与 Slint 版一致）；路径保留上次输入。
  for (const id of ["schema-meta-id", "schema-meta-name", "schema-meta-category", "schema-meta-version"]) {
    $(id).value = "";
  }
  $("schema-preset-out").checked = false;
  try {
    schemaState.outline = await invoke("schema_outline");
    schemaState.selectedOut = new Set(
      schemaState.outline.flatMap((g) => g.nodes.map((n) => `${g.group}/${n.name}`)),
    );
    renderSchemaOutTree();
  } catch (e) {
    showFormError($("schema-error"), String(e));
  }
}

async function runSchema() {
  $("schema-error").hidden = true;
  try {
    if (schemaState.mode === "import") {
      const path = $("schema-in").value.trim();
      if (!path) return showFormError($("schema-error"), "请选择 .tblschema 文件");
      if (!schemaState.preview) return showFormError($("schema-error"), "请先读取文件预览");
      if (!schemaState.selectedIn.size) return showFormError($("schema-error"), "至少选择一个节点");
      const snap = await invoke("schema_import", {
        path,
        withPreset: $("schema-preset-in").checked,
        sections: [...schemaState.selectedIn],
      });
      applySnapshot(snap);
      $("schema-result").hidden = false;
      $("schema-result").textContent = "导入完成";
      toast("Schema 导入完成", "ok");
      schemaState.preview = null;
      $("schema-in-body").hidden = true;
      $("schema-in-empty").hidden = false;
      return;
    }
    const asTemplate = schemaState.mode === "template";
    const keys = [...schemaState.selectedOut];
    if (!keys.length) return showFormError($("schema-error"), "至少选择一个节点");
    const id = $("schema-meta-id").value.trim();
    if (asTemplate && !id) return showFormError($("schema-error"), "模板 ID 必填");
    const path = $("schema-out").value.trim();
    if (!asTemplate && !path) return showFormError($("schema-error"), "请填写输出文件");
    const msg = await invoke("schema_export", {
      path: asTemplate ? "" : path,
      withPreset: $("schema-preset-out").checked,
      groups: [],
      nodes: keys,
      metaId: id,
      metaName: $("schema-meta-name").value.trim(),
      metaCategory: $("schema-meta-category").value.trim(),
      metaVersion: $("schema-meta-version").value.trim(),
      asTemplate,
    });
    $("schema-result").hidden = false;
    $("schema-result").textContent = msg;
    toast(msg, "ok");
    if (asTemplate) templatesCache = null; // 让新建项目的模板列表读到新模板
  } catch (e) {
    showFormError($("schema-error"), String(e));
  }
}

function initSchemaDialog() {
  $("schema-out-browse").addEventListener("click", async () => {
    const path = await invoke("pick_save", {
      title: "导出 Schema",
      defaultName: $("schema-out").value || "schema.tblschema",
      extensions: ["tblschema"],
    }).catch(() => null);
    if (path) $("schema-out").value = path;
  });
  $("schema-in-browse").addEventListener("click", async () => {
    const path = await pickPath("选择 .tblschema", "tblschema");
    if (path) {
      $("schema-in").value = path;
      loadSchemaImportPreview();
    }
  });
  $("schema-in").addEventListener("change", loadSchemaImportPreview);
  $("schema-close").addEventListener("click", () => ($("schema-dialog").hidden = true));
  $("schema-run").addEventListener("click", runSchema);

  $("schema-dialog").addEventListener("change", (e) => {
    const el = e.target;
    if (el.id === "schema-all-out" || el.id === "schema-all-in") {
      const out = el.id === "schema-all-out";
      const kind = out ? "out" : "in";
      const sel = out ? schemaState.selectedOut : schemaState.selectedIn;
      const groups = out ? schemaState.outline : schemaGroupsFromPreview();
      sel.clear();
      if (el.checked) {
        groups.forEach((g) => g.nodes.forEach((n) => sel.add(`${g.group}/${n.name}`)));
      }
      out ? renderSchemaOutTree() : renderSchemaInTree();
      return;
    }
    const sg = el.closest("[data-sg]");
    const sn = el.closest("[data-sn]");
    if (!sg && !sn) return;
    const out = !!el.closest("#schema-out-tree");
    const sel = out ? schemaState.selectedOut : schemaState.selectedIn;
    const groups = out ? schemaState.outline : schemaGroupsFromPreview();
    if (sg) {
      const g = groups.find((x) => x.group === sg.dataset.sg);
      if (!g) return;
      for (const n of g.nodes) {
        const key = `${g.group}/${n.name}`;
        el.checked ? sel.add(key) : sel.delete(key);
      }
      out ? renderSchemaOutTree() : renderSchemaInTree();
    } else {
      el.checked ? sel.add(sn.dataset.sn) : sel.delete(sn.dataset.sn);
      syncSchemaGroups(out ? "out" : "in");
      updateSchemaStats();
    }
  });
}

// ── Excel ───────────────────────────────────────────────────
let excelCtx = { kind: "export", group: "" };

function openExcelDialog(kind, group) {
  excelCtx = { kind, group };
  $("excel-title").textContent = `${kind === "export" ? "导出 Excel" : "导入 Excel"} — ${group}`;
  $("excel-path").value = `.excel/${group}.xlsx`;
  $("excel-open").hidden = kind !== "export";
  $("excel-result").hidden = true;
  $("excel-error").hidden = true;
  $("excel-dialog").hidden = false;
}

async function runExcel() {
  const path = $("excel-path").value.trim();
  if (!path) return showFormError($("excel-error"), "请填写文件路径");
  $("excel-error").hidden = true;
  try {
    if (excelCtx.kind === "export") {
      const msg = await invoke("excel_export", { group: excelCtx.group, path });
      $("excel-result").hidden = false;
      $("excel-result").textContent = msg;
      toast(msg, "ok");
      refreshExcelFiles();
    } else {
      const snap = await invoke("excel_import", { group: excelCtx.group, path });
      applySnapshot(snap);
      toast("Excel 导入完成", "ok");
    }
  } catch (e) {
    showFormError($("excel-error"), String(e));
  }
}

let excelFileCache = [];
let excelSelection = new Set();

function updateExcelSelection() {
  document.querySelectorAll("#excel-list .excel-card").forEach((c) =>
    c.classList.toggle("selected", excelSelection.has(c.dataset.file)),
  );
  $("excel-delete").disabled = excelSelection.size === 0;
}

function openExcelFile(file) {
  invoke("open_folder", { path: `.excel/${file}` }).catch((e) => addLocalLog(2, String(e)));
}

async function refreshExcelFiles() {
  try {
    const files = await invoke("excel_files");
    excelFileCache = files;
    excelSelection = new Set([...excelSelection].filter((f) => files.includes(f)));
    $("excel-list").innerHTML = files.length
      ? `<div class="excel-cards">${files
          .map(
            (f) =>
              `<div class="excel-card" data-file="${escapeHtml(
                f,
              )}"><span class="excel-icon">📄</span><span class="excel-card-name">${escapeHtml(
                f,
              )}</span></div>`,
          )
          .join("")}</div>`
      : '<div class="sys-empty">(无 Excel 文件)</div>';
    updateExcelSelection();
    $("excel-dir").textContent = ".excel/";
  } catch (e) {
    addLocalLog(2, String(e));
  }
}

async function tryDeleteExcelFiles() {
  if (!excelSelection.size) return;
  const names = [...excelSelection];
  const ok = await openModal({
    title: "删除 Excel 文件",
    message: `确定删除 ${names.join("、")}？只删 .excel 下的工作簿，不影响已导入的数据。`,
    input: false,
  });
  if (!ok) return;
  try {
    for (const f of names) await invoke("excel_delete", { file: f });
    excelSelection.clear();
    await refreshExcelFiles();
    toast(`已删除 ${names.length} 个 Excel 文件`, "ok");
  } catch (e) {
    toast(String(e), "error");
  }
}

// ── Excel 同步面板（core excel_sync 计划）──────────────────
async function refreshExcelSync() {
  try {
    const rows = await invoke("excel_sync_plan");
    $("es-list").innerHTML =
      rows
        .map((r) => {
          const ops =
            r.ops
              .map(
                (o) =>
                  `<button class="mini-btn" data-es-op="${o.code}" data-es-group="${escapeHtml(
                    r.group,
                  )}" data-es-node="${escapeHtml(r.node)}" ${
                    o.code === 8 ? 'disabled title="该操作尚未实现"' : ""
                  }>${escapeHtml(o.label)}</button>`,
              )
              .join("") || "";
          const notes = [r.header_edited ? "表头 type/export 被改过" : "", r.outside ? "区域外有内容" : ""]
            .filter(Boolean)
            .join(" · ");
          return `<div class="es-row">
            <span class="es-col-g"><span class="es-name">${escapeHtml(r.group)}/${escapeHtml(
            r.node,
          )}</span>${r.desc_diff ? '<span class="es-dot" title="desc 有差异"></span>' : ""}${
            notes ? `<span class="es-note">${escapeHtml(notes)}</span>` : ""
          }</span>
            <span class="es-col-size">${r.left ? escapeHtml(r.left) : "(新节点)"}</span>
            <span class="es-col-ops">${ops || '<span class="es-blocked">—</span>'}</span>
            <span class="es-col-size">${r.right ? escapeHtml(r.right) : "(新 sheet)"}${
            r.blocked ? `<span class="es-blocked">${escapeHtml(r.blocked)}</span>` : ""
          }</span>
          </div>`;
        })
        .join("") || '<div class="sys-empty">没有可同步的组</div>';
  } catch (e) {
    toast(String(e), "error");
  }
}

function openExcelSyncDialog() {
  $("excel-sync-dialog").hidden = false;
  refreshExcelSync();
}

function initExcelDialog() {
  $("excel-close").addEventListener("click", () => ($("excel-dialog").hidden = true));
  $("excel-open").addEventListener("click", () => {
    const p = $("excel-path").value.trim();
    if (!p) return;
    invoke("open_folder", { path: p }).catch((e) => addLocalLog(2, String(e)));
  });
  $("excel-run").addEventListener("click", runExcel);
  $("excel-browse").addEventListener("click", async () => {
    if (excelCtx.kind === "export") {
      const path = await invoke("pick_save", {
        title: "导出 Excel",
        defaultName: `${excelCtx.group}.xlsx`,
        extensions: ["xlsx"],
      }).catch(() => null);
      if (path) $("excel-path").value = path;
    } else {
      const path = await pickPath("选择 Excel 工作簿", "xlsx");
      if (path) $("excel-path").value = path;
    }
  });
  $("excel-refresh").addEventListener("click", refreshExcelFiles);
  $("excel-list").addEventListener("click", (e) => {
    const card = e.target.closest(".excel-card[data-file]");
    if (!card) return;
    const file = card.dataset.file;
    if (e.ctrlKey || e.metaKey) {
      if (excelSelection.has(file)) excelSelection.delete(file);
      else excelSelection.add(file);
    } else {
      excelSelection = new Set([file]);
    }
    updateExcelSelection();
  });
  $("excel-list").addEventListener("dblclick", (e) => {
    const card = e.target.closest(".excel-card[data-file]");
    if (card) openExcelFile(card.dataset.file);
  });
  $("excel-open-selected").addEventListener("click", () => {
    const list = excelSelection.size ? [...excelSelection] : excelFileCache.slice(0, 1);
    list.forEach(openExcelFile);
  });
  $("excel-delete").addEventListener("click", tryDeleteExcelFiles);
  $("excel-sync").addEventListener("click", openExcelSyncDialog);
  $("es-refresh").addEventListener("click", refreshExcelSync);
  $("es-close").addEventListener("click", () => ($("excel-sync-dialog").hidden = true));
  $("es-list").addEventListener("click", async (e) => {
    const b = e.target.closest("[data-es-op]");
    if (!b) return;
    try {
      const snap = await invoke("excel_sync_action", {
        group: b.dataset.esGroup,
        node: b.dataset.esNode,
        code: Number(b.dataset.esOp),
      });
      applySnapshot(snap);
      toast("已同步", "ok");
      await refreshExcelSync();
      await refreshExcelFiles();
    } catch (err) {
      toast(String(err), "error");
    }
  });
  $("excel-open-folder").addEventListener("click", () =>
    invoke("open_folder", { path: null }).catch((e) => addLocalLog(2, String(e))),
  );
  $("btn-excel").addEventListener("click", () => {
    const sel = state.selected;
    if (!sel || !sel.group) return addLocalLog(1, "请先选择一个分组或节点");
    excelEdit(sel.project, sel.group);
  });
}

let emPreview = null;

function openExcelMap() {
  emPreview = null;
  $("em-path").value = "";
  $("em-sheet").innerHTML = "";
  $("em-header").value = 1;
  $("em-start").value = 2;
  $("em-group").value = "";
  $("em-node").value = "";
  $("em-fields").innerHTML = "";
  $("em-preview").innerHTML = "";
  $("em-result").hidden = true;
  $("em-error").hidden = true;
  $("excel-map").hidden = false;
}

function pascalName(raw) {
  const s = (raw || "").replace(/[^A-Za-z0-9_]/g, "");
  if (!s) return "Imported";
  return s[0].toUpperCase() + s.slice(1);
}

async function loadExcelPreview() {
  const path = $("em-path").value.trim();
  if (!path) return showFormError($("em-error"), "请填写 .xlsx 路径");
  $("em-error").hidden = true;
  try {
    const p = await invoke("excel_preview", {
      path,
      sheet: $("em-sheet").value || null,
      headerRow: Number($("em-header").value) || null,
    });
    emPreview = p;
    $("em-sheet").innerHTML = p.sheets
      .map((s) => `<option${s === p.sheet ? " selected" : ""}>${escapeHtml(s)}</option>`)
      .join("");
    $("em-header").value = p.header_row;
    $("em-start").value = p.data_start;
    if (!$("em-group").value) {
      $("em-group").value = (p.sheet || "imported").toLowerCase().replace(/[^a-z0-9_]/g, "_");
    }
    if (!$("em-node").value) $("em-node").value = pascalName(p.sheet);
    renderEmPreview();
    renderEmFields();
  } catch (e) {
    showFormError($("em-error"), String(e));
  }
}

/** 原始 Excel 预览（表头 + 前几行），对应 Slint 映射对话框的「原始数据」页。 */
function renderEmPreview() {
  if (!emPreview) return;
  const cols = emPreview.columns || [];
  const rows = emPreview.sample_rows || [];
  $("em-preview").innerHTML =
    `<table><thead><tr><th>#</th>${cols
      .map((c) => `<th>${escapeHtml(c)}</th>`)
      .join("")}</tr></thead><tbody>` +
    rows
      .map(
        (r, i) =>
          `<tr><th>${i + 1}</th>${cols
            .map((_, c) => `<td>${escapeHtml(r[c] ?? "")}</td>`)
            .join("")}</tr>`,
      )
      .join("") +
    "</tbody></table>";
  renderEmFinal();
}

/** 按当前映射生成最终 4 行表头 + 数据预览。 */
function renderEmFinal() {
  if (!emPreview) return;
  const fields = emPreview.fields || [];
  const rows = emPreview.sample_rows || [];
  const head = [
    fields.map((f) => f.desc),
    fields.map((f) => f.export),
    fields.map((f) => f.tbl_type),
    fields.map((f) => f.target_name),
  ];
  $("em-final").innerHTML =
    `<table><thead>${head
      .map(
        (r, i) =>
          `<tr>${r.map((v) => `<th class="${i === 3 ? "em-fin-name" : ""}">${escapeHtml(v)}</th>`).join("")}</tr>`,
      )
      .join("")}</thead><tbody>` +
    rows
      .map(
        (r) =>
          `<tr>${fields
            .map((f) => `<td>${escapeHtml(r[f.source_col - 1] ?? "")}</td>`)
            .join("")}</tr>`,
      )
      .join("") +
    "</tbody></table>";
}

const EM_TYPES = ["int", "long", "float", "double", "bool", "str", "txt"];
const EM_EXPORTS = ["前后端", "客户端", "服务器", "不导出"];

/** 映射字段可编辑（目标名 / 源列 / 类型 / 导出 / 说明），不再只是只读建议。 */
function renderEmFields() {
  if (!emPreview) return;
  const cols = emPreview.columns || [];
  $("em-fields").innerHTML = emPreview.fields
    .map((f, i) => {
      const src = cols
        .map(
          (c, j) =>
            `<option value="${j + 1}"${j + 1 === f.source_col ? " selected" : ""}>${escapeHtml(
              `${j + 1}. ${c}`,
            )}</option>`,
        )
        .join("");
      const types = EM_TYPES.includes(f.tbl_type) ? EM_TYPES : [f.tbl_type, ...EM_TYPES];
      return (
        `<div class="em-field" data-em-row="${i}">` +
        `<span class="col">${i + 1}</span>` +
        `<input class="input em-name" data-em-key="target_name" value="${escapeHtml(f.target_name)}" />` +
        `<select class="combo em-src" data-em-key="source_col">${src}</select>` +
        `<select class="combo em-type" data-em-key="tbl_type">${types
          .map((t) => `<option${t === f.tbl_type ? " selected" : ""}>${escapeHtml(t)}</option>`)
          .join("")}</select>` +
        `<select class="combo em-export" data-em-key="export">${EM_EXPORTS.map(
          (x) => `<option${x === f.export ? " selected" : ""}>${x}</option>`,
        ).join("")}</select>` +
        `<input class="input em-desc" data-em-key="desc" value="${escapeHtml(f.desc)}" placeholder="说明" />` +
        `<button class="mini-btn" data-em-remove="${i}" title="删除该字段">×</button>` +
        "</div>"
      );
    })
    .join("");
}

function emFieldRow(el) {
  const row = el.closest("[data-em-row]");
  if (!row || !emPreview) return null;
  const f = emPreview.fields[+row.dataset.emRow];
  return f ? { row, f } : null;
}

async function importAllExcelSheets() {
  if (!emPreview) return showFormError($("em-error"), "请先读取预览");
  const path = $("em-path").value.trim();
  const group = $("em-group").value.trim();
  if (!path || !group) return showFormError($("em-error"), "请填写文件和 Group");
  $("em-error").hidden = true;
  let snap = null;
  const failed = [];
  for (const sheet of emPreview.sheets) {
    try {
      const p = await invoke("excel_preview", {
        path,
        sheet,
        headerRow: Number($("em-header").value) || null,
      });
      snap = await invoke("excel_import_mapped", {
        request: {
          path,
          sheet,
          header_row: p.header_row,
          data_start: p.data_start,
          group_name: group,
          node_name: pascalName(sheet),
          fields: p.fields,
        },
      });
    } catch (e) {
      failed.push(`${sheet}: ${e}`);
    }
  }
  if (snap) applySnapshot(snap);
  $("em-result").hidden = false;
  $("em-result").textContent =
    `导入全部完成（${emPreview.sheets.length - failed.length}/${emPreview.sheets.length}）` +
    (failed.length ? `\n失败：\n${failed.join("\n")}` : "");
  toast(`已导入 ${emPreview.sheets.length - failed.length} 个 Sheet`, failed.length ? "error" : "ok");
}

async function runExcelMap() {
  if (!emPreview) return showFormError($("em-error"), "请先读取预览");
  $("em-error").hidden = true;
  try {
    const snap = await invoke("excel_import_mapped", {
      request: {
        path: $("em-path").value.trim(),
        sheet: $("em-sheet").value,
        header_row: Number($("em-header").value),
        data_start: Number($("em-start").value),
        group_name: $("em-group").value.trim(),
        node_name: $("em-node").value.trim(),
        fields: emPreview.fields,
      },
    });
    applySnapshot(snap);
    $("em-result").hidden = false;
    $("em-result").textContent = "导入完成";
    toast("Excel 映射导入完成", "ok");
  } catch (e) {
    showFormError($("em-error"), String(e));
  }
}

function initExcelMap() {
  $("em-browse").addEventListener("click", async () => {
    const path = await pickPath("选择 Excel 工作簿", "xlsx");
    if (path) {
      $("em-path").value = path;
      loadExcelPreview();
    }
  });
  $("em-close").addEventListener("click", () => ($("excel-map").hidden = true));
  $("em-load").addEventListener("click", loadExcelPreview);
  $("em-run").addEventListener("click", runExcelMap);
  $("em-import-all").addEventListener("click", importAllExcelSheets);
  $("em-sheet").addEventListener("change", loadExcelPreview);
  $("em-header").addEventListener("change", loadExcelPreview);
  $("em-start").addEventListener("change", loadExcelPreview);
  $("em-fields").addEventListener("input", (e) => {
    const hit = emFieldRow(e.target);
    if (!hit) return;
    const key = e.target.dataset.emKey;
    hit.f[key] = key === "source_col" ? Number(e.target.value) : e.target.value;
    renderEmFinal();
  });
  $("em-fields").addEventListener("change", (e) => {
    const hit = emFieldRow(e.target);
    if (!hit) return;
    const key = e.target.dataset.emKey;
    hit.f[key] = key === "source_col" ? Number(e.target.value) : e.target.value;
    renderEmFinal();
  });
  $("em-fields").addEventListener("click", (e) => {
    const del = e.target.closest("[data-em-remove]");
    if (!del || !emPreview) return;
    emPreview.fields.splice(Number(del.dataset.emRemove), 1);
    renderEmFields();
    renderEmFinal();
  });
  $("em-add-field").addEventListener("click", () => {
    if (!emPreview) return;
    const n = emPreview.fields.length + 1;
    emPreview.fields.push({
      target_name: `field_${n}`,
      source_col: Math.min(n, emPreview.columns.length || n),
      tbl_type: "str",
      export: "前后端",
      desc: "",
    });
    renderEmFields();
    renderEmFinal();
  });
}

// ── 校验问题（可跳转）───────────────────────────────────────
function renderIssues() {
  const issues = state.snapshot?.issues || [];
  state.issues = issues;
  $("issue-summary").textContent = `${issues.length} 条`;
  $("issue-list").innerHTML = issues.length
    ? issues
        .map(
          (it, i) =>
            `<div class="issue-row ${it.severity}" data-issue="${i}">` +
            `<span class="sev">${it.severity === "error" ? "错误" : it.severity === "warn" ? "警告" : "信息"}</span>` +
            `<span class="loc">${escapeHtml(it.group)}/${escapeHtml(it.name)}${
              it.jump ? ` ${it.row + 1}:${it.col + 1}` : ""
            }</span>` +
            `<span class="msg" title="${escapeHtml(it.message)}">${escapeHtml(it.message)}</span></div>`,
        )
        .join("")
    : '<div class="sys-empty">没有校验问题</div>';
}

async function jumpIssue(index) {
  const it = (state.issues || [])[index];
  if (!it) return;
  const row = state.snapshot.tree.find((r) => r.group === it.group && r.name === it.name);
  if (!row) return;
  state.selected = { project: row.project, kind: row.kind, group: it.group, name: it.name };
  state.expandedProjects.add(row.project);
  state.expandedGroups.add(`${row.project}|${it.group}`);
  renderTree();
  await runOp("select_node", {
    project: row.project,
    kind: row.kind,
    group: it.group,
    name: it.name,
  });
  if (it.jump) {
    try {
      grid.selectCell(it.row, it.col, false);
    } catch {}
  }
}

function initIssues() {
  $("issue-list").addEventListener("click", (e) => {
    const row = e.target.closest("[data-issue]");
    if (row) jumpIssue(Number(row.dataset.issue));
  });
  $("issue-refresh").addEventListener("click", () => runOp("validate_workspace", {}));
}

// ── 关于 ────────────────────────────────────────────────────
function openAbout() {
  const s = state.snapshot;
  const rows = [
    ["版本", `tablet v${s?.version || "?"}`],
    ["目标平台", s?.target || "-"],
    ["工作区", s?.workdir || "-"],
    ["版本库", s?.vcs || "无版本库"],
  ];
  $("about-rows").innerHTML = rows
    .map(
      ([k, v]) =>
        `<div class="row"><span class="k">${k}</span><span class="v">${escapeHtml(v)}</span></div>`,
    )
    .join("");
  $("about-dialog").hidden = false;
}

function initAbout() {
  $("btn-about").addEventListener("click", openAbout);
  $("about-close").addEventListener("click", () => ($("about-dialog").hidden = true));
  $("about-update").addEventListener("click", () =>
    invoke("open_url", { url: "https://github.com/juebanlin/tablet" }).catch((e) =>
      addLocalLog(2, String(e)),
    ),
  );
}

// ── 启动 ────────────────────────────────────────────────────
function boot() {
  syncSystemTheme();
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", syncSystemTheme);
  grid = new SheetGrid({
    scroll: $("grid-scroll"),
    table: $("sheet"),
    onSelection: onGridSelection,
    onCommit: (ed, value) => {
      if (ed.type === "cell") fastCommitCell(ed.r, ed.c, value);
      else if (ed.type === "header") runOp("commit_header", { headerRow: ed.hi, col: ed.ci, value });
      else if (ed.type === "clear") runOp("clear_cells", { cells: ed.cells });
      else if (ed.type === "paste") runOp("paste_cells", { row: ed.r, col: ed.c, cells: ed.grid });
    },
    onLog: addLocalLog,
    onContextMenu: (ctx) => showContextMenu(ctx.x, ctx.y, gridMenu(ctx)),
    onRefPicker: (target, strategy) => invoke("ref_picker", { target, strategy: strategy || null }),
    onTypeSelector: (target) => openTypeSelector(target),
    onViewChange: (view) => saveGridView(view),
  });

  initToolbar();
  initBottomPanel();
  initFormulaBar();
  initSplitters();
  initModals();
  initTypeSelector();
  initPasteGroupDialog();
  initProjectDialogs();
  initRulesEditor();
  initSchemaDialog();
  initExcelDialog();
  initExcelMap();
  initIssues();
  initAbout();
  initGlobalSettings();
  initExportDialog();
  initContextMenus();

  $("tree-list").addEventListener("click", onTreeClick);
  // 双击项目 / 组行展开收起（与 Slint 版一致）。
  $("tree-list").addEventListener("dblclick", (e) => {
    const item = e.target.closest(".tree-item");
    if (!item) return;
    const t = decodeTreeKey(item.dataset.key);
    if (t.kind === "project") {
      if (!state.expandedProjects.has(t.project)) state.expandedProjects.add(t.project);
      else state.expandedProjects.delete(t.project);
    } else if (t.kind === "group") {
      const key = `${t.project}|${t.group}`;
      if (!state.expandedGroups.has(key)) state.expandedGroups.add(key);
      else state.expandedGroups.delete(key);
    } else return;
    renderTree();
  });
  $("tree-search").addEventListener("input", (e) => {
    state.treeSearch = e.target.value;
    renderTree();
  });
  $("tree-filter-select").addEventListener("change", (e) => {
    state.treeFilter = e.target.selectedIndex;
    renderTree();
  });
  $("sort-select").addEventListener("change", (e) => {
    state.treeSort = e.target.selectedIndex;
    renderTree();
    const sort = ["id", "name", "open", "created", "manual"][state.treeSort] || "id";
    invoke("set_project_sort", { sort }).catch((err) => toast(String(err), "error"));
  });
  $("full-group").addEventListener("change", (e) => {
    state.fullGroup = e.target.checked;
    renderTree();
  });
  $("tree-show-id").addEventListener("change", (e) => {
    // 与「全局设置 → 项目列表显示元数据 ID」是同一项设置；共用 core/后端持久化。
    runOp("set_show_meta_id", { on: e.target.checked }).catch(() => {});
  });

  window.__tt = { grid, state, runOp, applySnapshot, selectTree };

  // WebView2 启动时可能把焦点落到第一个可用按钮上，别让焦点环一直挂着。
  if (document.activeElement?.tagName === "BUTTON") document.activeElement.blur();

  runOp("snapshot", {}).then(() => {
    addLocalLog(0, "工作区已加载");
  });

  // 主窗口关闭：有未保存改动先确认（Slint 版的同款保护）。
  try {
    const mainWin = window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.();
    mainWin?.onCloseRequested?.((event) => {
      if (!status().dirty) return;
      event.preventDefault();
      if (confirm("工作区有未保存的改动，关闭将丢弃。确定关闭？")) {
        mainWin.destroy();
      }
    });
  } catch {}

  // Univer 窗口回写内存后通知主界面刷新；窗口重新获得焦点也补一次（事件不可用时的兜底）。
  try {
    window.__TAURI__?.event?.listen("tablet://state-changed", () => {
      invoke("snapshot").then(applySnapshot).catch(() => {});
    });
  } catch {}
  window.addEventListener("focus", () => {
    invoke("snapshot").then(applySnapshot).catch(() => {});
  });

  // Excel 编辑会话轮询：Excel 保存后 mtime 变化 → 自动导入回内存。
  setInterval(async () => {
    try {
      const changed = await invoke("excel_poll");
      for (const it of changed) {
        try {
          const snap = await invoke("excel_sync_group", {
            project: it.project,
            group: it.group,
          });
          applySnapshot(snap);
          toast(`Excel 已回写：${it.group}`, "ok");
        } catch {
          // Excel 可能正在写盘，下一轮重试。
        }
      }
    } catch {}
    // xlsx 组外部修改：干净就自动重载；本地有脏改动则提示一次并跳过。
    try {
      const changes = await invoke("xlsx_poll");
      for (const it of changes) {
        const key = `${it.project}|${it.group}`;
        if (it.dirty) {
          if (!state.xlsxWarned.has(key)) {
            state.xlsxWarned.add(key);
            addLocalLog(2, `[${it.group}] 外部工作簿已修改，本地有未保存改动，已跳过重载`);
            toast(`${it.group} 的外部工作簿已修改；本地有未保存改动，请先保存或还原`, "error");
          }
          continue;
        }
        if (grid.editing) continue; // 正在输入单元格，下一轮再重载
        state.xlsxWarned.delete(key);
        try {
          const snap = await invoke("reload_xlsx_external", {
            project: it.project,
            group: it.group,
          });
          applySnapshot(snap);
          toast(`已重新加载外部工作簿：${it.group}`, "ok");
        } catch {
          // 外部可能正在写盘，下一轮重试。
        }
      }
    } catch {}
  }, 2000);
}

boot();
