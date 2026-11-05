// 表格交互层：选择 / 编辑 / 键盘 / 剪贴板 / 列宽。
//
// 需求是从 Slint 版反推出来的（@docs/04 §3–§4），实现方式按 Web 习惯重做：
//   - 表头、行号固定，数据区双向滚动（CSS sticky）
//   - 单格 / 矩形 / 整行 / 整列选择，支持拖选与 Shift 扩选
//   - 双击、F2、直接打字进入编辑；Enter / Tab / Esc / 方向键都有明确语义
//   - Ctrl+C/X/V、Delete 走 TSV
//   - 列宽拖拽 + 双击自适应
// 选择变化只改 class，不重建 DOM —— 拖选时不能卡。

const ALPHA = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/** 列序号 → Excel 列字母：0=A、25=Z、26=AA……与 Slint 版 col_letter 一致。 */
function colLetter(idx) {
  let s = "";
  let n = Math.max(0, idx | 0);
  for (;;) {
    s = ALPHA[n % 26] + s;
    if (n < 26) break;
    n = Math.floor(n / 26) - 1;
  }
  return s;
}

function escAttr(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

/** 类型下拉：基础 / 常用容器 / 当前项目的引用目标 / 自定义。 */
function typeOptionsHtml(current, refs) {
  const groups = [
    ["基础", ["int", "long", "float", "double", "str", "bool", "txt"]],
    [
      "容器",
      [
        "List<int>", "List<str>", "List<float>", "List<double>",
        "Set<int>", "Set<str>",
        "Map<str,int>", "Map<int,str>", "Map<str,str>",
        "Tuple2<int,int>", "Tuple3<int,int,int>", "Tuple4<int,int,int,int>",
      ],
    ],
    ["引用", (refs || []).map((r) => "@" + r)],
  ];
  const seen = new Set();
  let html = "";
  for (const [label, items] of groups) {
    if (!items.length) continue;
    html += `<optgroup label="${escAttr(label)}">`;
    for (const it of items) {
      seen.add(it);
      html += `<option value="${escAttr(it)}"${it === current ? " selected" : ""}>${escAttr(it)}</option>`;
    }
    html += "</optgroup>";
  }
  if (current && !seen.has(current)) {
    html += `<option value="${escAttr(current)}" selected>${escAttr(current)}</option>`;
  }
  html += '<option value="__custom__">自定义…</option>';
  return html;
}

function clamp(n, lo, hi) {
  return Math.max(lo, Math.min(n, hi));
}

class SheetGrid {
  constructor({ scroll, table, onSelection, onCommit, onLog, onContextMenu, onRefPicker, onTypeSelector, onViewChange }) {
    this.scroll = scroll;
    this.table = table;
    this.onSelection = onSelection || (() => {});
    this.onCommit = onCommit || (() => {});
    this.onLog = onLog || (() => {});
    this.onContextMenu = onContextMenu || (() => {});
    this.onRefPicker = onRefPicker || (() => Promise.resolve(null));
    this.onTypeSelector = onTypeSelector || (() => Promise.resolve(null));
    this.onViewChange = onViewChange || (() => {});
    this._clip = "";
    this._refPopup = null;
    this._refOutside = null;
    this._headerPopup = null;
    this._headerOutside = null;

    this.node = null;
    this.nodeKey = "";
    // Univer 窗口打开该组时，主界面只读（防止两处同时改同一份数据）。
    this.locked = false;
    this.colWidths = [];
    this.widthsByKey = new Map();
    this.autoWidth = true;
    // 失焦是否提交（全局设置）；关闭时失焦丢弃当前编辑。
    this.autoCommitOnBlur = true;

    this.tds = [];
    this.rownums = [];
    this.colheads = [];
    this.headerCells = [];

    this.anchor = { r: 0, c: 0 };
    this.focus = { r: 0, c: 0 };
    this.kind = "none"; // none | cell | range | row | col | all
    this.editing = null; // { type:'cell'|'header', r/hi, c/ci, el, original }
    this.dragging = false;
    this.dragAxis = null; // row / col 表头拖动扩选
    this.hover = null;
    this._lastBounds = null;
    this._dragPoint = null;
    this._raf = 0;

    this._bind();
  }

  // ── 数据 ────────────────────────────────────────────────
  // Univer 编辑锁：只拦截编辑入口，选择 / 滚动 / 复制仍可用。
  setLocked(on) {
    const next = !!on;
    if (this.locked === next) return;
    this.locked = next;
    if (next && this.editing) this.cancelEdit();
    this.scroll.classList.toggle("grid-locked", next);
  }

  setAutoCommitOnBlur(on) {
    this.autoCommitOnBlur = on !== false;
  }

  setData(node, opts = {}) {
    // 后端快照刷新：编辑中的输入框直接丢弃，不能把本地编辑再提交回去（会形成回环）。
    if (this.editing) this.cancelEdit();
    this.closeRefPicker();
    this.closeHeaderPicker();
    const prev = this.node;
    const sameNode = !!prev && !!node && prev.title === node.title;
    const keep =
      opts.keepSelection && sameNode
        ? { anchor: { ...this.anchor }, focus: { ...this.focus }, kind: this.kind }
        : null;
    this.node = node;
    this.hover = null;
    const cols = node ? (node.widths?.length || node.headers?.[0]?.length || 0) : 0;
    this.nodeKey = node
      ? `${node.mode}|${node.title}|${(node.headers?.[node.headers.length - 1] || []).join(",")}`
      : "";
    const cached = node ? this.widthsByKey.get(this.nodeKey) : null;
    this.colWidths = new Array(cols).fill(0);
    this.errorSet = new Set((node?.errors || []).map(([r, c]) => `${r}:${c}`));
    if (node) {
      this.autoWidth = node.autoWidth !== false;
      for (let c = 0; c < cols; c++) this.colWidths[c] = this.autoFitWidth(c);
      if (!this.autoWidth) {
        const names = node.headers?.[node.headers.length - 1] || [];
        const saved = node.columnWidths || {};
        for (let c = 0; c < cols; c++) {
          const w = cached?.[c] ?? saved[names[c]];
          if (Number.isFinite(w)) this.colWidths[c] = clamp(Math.round(w), 40, 800);
        }
      }
      if (cached && !this.autoWidth) this.colWidths = cached.slice();
    }
    this.render();

    // 切节点：清空选区并回到左上角（与 Slint 版一致）。
    if (!sameNode) {
      this.kind = "none";
      this.anchor = { r: 0, c: 0 };
      this.focus = { r: 0, c: 0 };
      this._lastBounds = null;
      this.scroll.scrollTop = 0;
      this.scroll.scrollLeft = 0;
    }
    if (keep && node && this.kind !== "none") {
      this.anchor = {
        r: Math.min(keep.anchor.r, this.rows - 1),
        c: Math.min(keep.anchor.c, this.cols - 1),
      };
      this.focus = {
        r: Math.min(keep.focus.r, this.rows - 1),
        c: Math.min(keep.focus.c, this.cols - 1),
      };
      this.kind = keep.kind === "none" ? "cell" : keep.kind;
      this._lastBounds = null;
      this.applySelection();
    }
    this.notify();
  }

  cssRowHeight() {
    const v = getComputedStyle(document.documentElement).getPropertyValue("--row-h").trim();
    const n = parseInt(v, 10);
    return Number.isFinite(n) && n > 0 ? n : 24;
  }

  get cols() {
    if (!this.node) return 0;
    if (this.node.widths) return this.node.widths.length;
    return this.node.headers?.[0]?.length || 0;
  }

  get rows() {
    return this.node ? this.node.rows.length : 0;
  }

  cellText(r, c) {
    return this.node?.rows[r]?.[c] ?? "";
  }

  /** 界面显示值（引用列可能显示名字）；存储/复制/编辑一律用 cellText。 */
  displayCellText(r, c) {
    return this.node?.displayRows?.[r]?.[c] ?? this.cellText(r, c);
  }

  setCellText(r, c, text) {
    if (this.locked) return false;
    if (!this.node || this.node.kinds[c] === "readonly") return false;
    if (this.cellText(r, c) === text) return false;
    this.node.rows[r][c] = text;
    return true;
  }

  setCellValue(r, c, value) {
    if (this.setCellText(r, c, value)) {
      this.paintCell(this.tds[r][c], r, c);
      this.onCommit({ type: "cell", r, c }, value);
      this.notify();
    }
  }

  /**
   * 引用选择器（对齐 Slint 版）：id + 辅助列的表格式列表。
   * 表可切换「智能 / 全部」列展示；单值单击选中、双击/确定提交；列表引用多选后拼 `;`。
   */
  async openRefPicker(r, c, target, multi = false) {
    this.closeRefPicker();
    const td = this.tds[r]?.[c];
    if (!td) return;
    const rect = td.getBoundingClientRect();
    const raw = this.cellText(r, c);
    const sep = ";";
    let data;
    try {
      data = await this.onRefPicker(target, null);
    } catch (e) {
      this.onLog(2, String(e));
      return;
    }
    if (!data) return;
    const state = {
      strategy: data.default_strategy || "auto",
      manual: raw,
      selectedId: multi ? "" : raw,
      chosen: new Set(multi ? raw.split(sep).map((s) => s.trim()).filter(Boolean) : []),
    };
    const width = Math.min(660, window.innerWidth - 16);
    const popup = document.createElement("div");
    popup.className = "ref-popup ref-picker";
    popup.style.width = `${width}px`;
    popup.style.left = `${Math.max(4, Math.min(rect.left, window.innerWidth - width - 4))}px`;
    popup.style.top = `${Math.max(4, Math.min(rect.bottom + 2, window.innerHeight - 380))}px`;
    popup.innerHTML = `
      <div class="ref-head">
        <span class="ref-target">@${escAttr(target)}</span>
        <span class="ref-badge" data-ref-kind></span>
        <span class="ref-strategy" data-ref-strategy>
          <span class="ref-hint">列展示</span>
          <button class="mini-btn" data-strategy="auto">智能</button>
          <button class="mini-btn" data-strategy="full">全部</button>
        </span>
      </div>
      <div class="ref-line"><label>当前值</label><input class="input" data-ref-manual placeholder="可直接输入，或在下方列表选择" /></div>
      <div class="ref-missing" data-ref-missing hidden></div>
      <div class="ref-line"><label>搜索</label><input class="input" data-ref-search placeholder="搜索 id / 任意列" /></div>
      <div class="ref-table-wrap"><table class="ref-table"><thead></thead><tbody></tbody></table></div>
      <div class="ref-foot">
        <span class="ref-preview" data-ref-preview></span>
        <span class="spacer"></span>
        <button class="mini-btn" data-ref-clear>清空</button>
        <button class="mini-btn" data-ref-cancel>取消</button>
        <button class="mini-btn primary" data-ref-ok>确定</button>
      </div>`;
    const q = (sel) => popup.querySelector(sel);
    const manualEl = q("[data-ref-manual]");
    const searchEl = q("[data-ref-search]");
    manualEl.value = raw;

    const filtered = () => {
      const term = searchEl.value.trim().toLowerCase();
      if (!term) return data.rows;
      return data.rows.filter(
        (row) =>
          row.id.toLowerCase().includes(term) ||
          row.extras.some((v) => (v || "").toLowerCase().includes(term)),
      );
    };
    const preview = () => {
      if (multi) {
        return state.chosen.size
          ? `已选 ${state.chosen.size} 项：${[...state.chosen].join(sep)}`
          : "（未选择）";
      }
      if (!state.selectedId) return "（未选择）";
      const row = data.rows.find((x) => x.id === state.selectedId);
      const extra = (row?.extras || []).find((v) => v) || "";
      return extra ? `${state.selectedId}（${extra}）` : state.selectedId;
    };
    const renderChrome = () => {
      q("[data-ref-kind]").textContent = data.is_table ? "表" : "枚举";
      q("[data-ref-strategy]").hidden = !data.is_table;
      popup
        .querySelectorAll("[data-strategy]")
        .forEach((b) => b.classList.toggle("primary", b.dataset.strategy === state.strategy));
      const miss = q("[data-ref-missing]");
      miss.hidden = !data.target_missing;
      miss.textContent = data.target_missing ? `引用的配置项「${target}」不存在` : "";
      q("[data-ref-ok]").disabled = !!data.target_missing;
      manualEl.disabled = !!data.target_missing;
    };
    const renderHead = () => {
      const desc = data.has_desc
        ? `<tr class="ref-desc"><th class="ref-idcol">${escAttr(
            data.id_header.desc || "",
          )}</th>${data.headers.map((h) => `<th>${escAttr(h.desc || "")}</th>`).join("")}</tr>`
        : "";
      q("thead").innerHTML =
        desc +
        `<tr class="ref-fields"><th class="ref-idcol">${escAttr(
          data.id_header.field || "id",
        )}</th>${data.headers.map((h) => `<th>${escAttr(h.field)}</th>`).join("")}</tr>`;
    };
    const renderBody = () => {
      const rows = filtered();
      q("tbody").innerHTML =
        rows
          .map((row) => {
            const on = multi ? state.chosen.has(row.id) : row.id === state.selectedId;
            return (
              `<tr class="ref-option${on ? " selected" : ""}" data-id="${escAttr(row.id)}">` +
              `<td class="ref-idcol">${
                multi ? `<span class="ref-check">${on ? "✓" : ""}</span>` : ""
              }<span class="ref-id">${escAttr(row.id)}</span></td>` +
              row.extras.map((v) => `<td title="${escAttr(v)}">${escAttr(v)}</td>`).join("") +
              `</tr>`
            );
          })
          .join("") ||
        `<tr><td class="ref-empty" colspan="${1 + data.headers.length}">无匹配</td></tr>`;
      q("[data-ref-preview]").textContent = preview();
    };
    const renderAll = () => {
      renderChrome();
      renderHead();
      renderBody();
    };
    renderAll();

    const pick = (id) => {
      if (multi) {
        if (state.chosen.has(id)) state.chosen.delete(id);
        else state.chosen.add(id);
        state.manual = [...state.chosen].join(sep);
        manualEl.value = state.manual;
      } else {
        state.selectedId = id;
        state.manual = id;
        manualEl.value = id;
      }
      renderBody();
    };
    const confirm = () => {
      const value = multi ? state.manual.trim() || [...state.chosen].join(sep) : state.manual.trim();
      this.setCellValue(r, c, value);
      this.closeRefPicker();
    };

    manualEl.addEventListener("input", () => {
      state.manual = manualEl.value;
      if (!multi) {
        state.selectedId = state.manual.trim();
        renderBody();
      }
    });
    manualEl.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") this.closeRefPicker();
      else if (e.key === "Enter") {
        e.preventDefault();
        confirm();
      }
    });
    searchEl.addEventListener("input", renderBody);
    searchEl.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") this.closeRefPicker();
    });
    q("tbody").addEventListener("click", (e) => {
      const tr = e.target.closest("tr[data-id]");
      if (tr) pick(tr.dataset.id);
    });
    q("tbody").addEventListener("dblclick", (e) => {
      const tr = e.target.closest("tr[data-id]");
      if (!tr) return;
      if (multi) pick(tr.dataset.id);
      else {
        state.selectedId = tr.dataset.id;
        state.manual = tr.dataset.id;
        confirm();
      }
    });
    q("[data-ref-ok]").addEventListener("click", confirm);
    q("[data-ref-cancel]").addEventListener("click", () => this.closeRefPicker());
    q("[data-ref-clear]").addEventListener("click", () => {
      this.setCellValue(r, c, "");
      this.closeRefPicker();
    });
    popup.querySelectorAll("[data-strategy]").forEach((b) =>
      b.addEventListener("click", async () => {
        const next = b.dataset.strategy;
        if (next === state.strategy) return;
        try {
          const fresh = await this.onRefPicker(target, next);
          if (!fresh) return;
          data = fresh;
          state.strategy = next;
          renderAll();
        } catch (err) {
          this.onLog(2, String(err));
        }
      }),
    );

    document.body.appendChild(popup);
    this._refPopup = popup;
    this._refOutside = (e) => {
      if (!popup.contains(e.target)) this.closeRefPicker();
    };
    setTimeout(() => document.addEventListener("pointerdown", this._refOutside, true), 0);
    searchEl.focus();
  }

  closeRefPicker() {
    if (this._refPopup) {
      this._refPopup.remove();
      this._refPopup = null;
    }
    if (this._refOutside) {
      document.removeEventListener("pointerdown", this._refOutside, true);
      this._refOutside = null;
    }
  }

  /** 表头 export / type 的直接下拉菜单（不是原生 select，点一下就展开）。 */
  openHeaderPicker(hi, ci, kind) {
    this.closeHeaderPicker();
    const th = this.headerCells[hi]?.[ci];
    if (!th) return;
    const rect = th.getBoundingClientRect();
    const current = this.node?.headers?.[hi]?.[ci] || "";
    // 类型走完整的「选择类型」对话框（范式/参数/预览/引用 tab），不是下拉菜单。
    if (kind === "type") {
      this.openTypeSelector({ kind: "header", hi, ci, current });
      return;
    }
    const popup = document.createElement("div");
    popup.className = "cell-menu";
    if (kind === "export") {
      const options = ["前后端", "客户端", "服务器", "不导出"];
      popup.innerHTML = options
        .map(
          (v) =>
            `<div class="cell-menu-item${v === current ? " active" : ""}" data-value="${escAttr(v)}">${escAttr(v)}</div>`,
        )
        .join("");
      popup.addEventListener("click", (e) => {
        const it = e.target.closest("[data-value]");
        if (!it) return;
        this.commitHeaderValue(hi, ci, it.dataset.value);
        this.closeHeaderPicker();
      });
    }
    popup.style.left = `${Math.max(4, Math.min(rect.left, window.innerWidth - 240))}px`;
    popup.style.top = `${Math.max(4, Math.min(rect.bottom + 2, window.innerHeight - 320))}px`;
    document.body.appendChild(popup);
    this._headerPopup = popup;
    this._headerOutside = (e) => {
      if (!popup.contains(e.target)) this.closeHeaderPicker();
    };
    setTimeout(() => document.addEventListener("pointerdown", this._headerOutside, true), 0);
  }

  closeHeaderPicker() {
    if (this._headerPopup) {
      this._headerPopup.remove();
      this._headerPopup = null;
    }
    if (this._headerOutside) {
      document.removeEventListener("pointerdown", this._headerOutside, true);
      this._headerOutside = null;
    }
  }

  commitHeaderValue(hi, ci, value) {
    if (!this.node) return;
    this.node.headers[hi][ci] = value;
    this.paintHeader(this.headerCells[hi][ci], hi, ci);
    this.onCommit({ type: "header", hi, ci }, value);
  }

  isReadonlyCol(c) {
    return (this.node?.kinds[c] || "text") === "readonly";
  }

  /** 该列是否只能通过选择器改（引用 / 类型 / 导出），公式栏必须只读。 */
  isPickerCol(c) {
    return ["ref", "type", "export"].includes(this.node?.kinds[c] || "");
  }

  isNumericCol(c) {
    const typeRow = this.node?.headers?.find((h) => h.length && /^(int|long|float|double)$/.test(h[c]));
    return !!typeRow;
  }

  // ── 渲染 ────────────────────────────────────────────────
  render() {
    this.table.innerHTML = "";
    this.tds = [];
    this.rownums = [];
    this.colheads = [];
    this.headerCells = [];
    if (!this.node) {
      this.table.hidden = true;
      return;
    }
    this.table.hidden = false;

    const n = this.node;
    const cols = this.cols;

    // 列宽
    const colgroup = document.createElement("colgroup");
    const gutter = document.createElement("col");
    gutter.style.width = "32px";
    colgroup.appendChild(gutter);
    const colEls = [];
    for (let c = 0; c < cols; c++) {
      const col = document.createElement("col");
      col.style.width = this.colWidths[c] + "px";
      colEls.push(col);
      colgroup.appendChild(col);
    }
    this.colEls = colEls;
    this.table.appendChild(colgroup);

    // 表头
    const thead = document.createElement("thead");
    // 行高由 CSS 变量 --row-h 决定，表头 sticky 偏移必须跟着它走。
    const rowH = this.cssRowHeight();
    const topOffset = rowH;
    this.rowH = rowH;

    const letterRow = document.createElement("tr");
    letterRow.className = "letters-row";
    const corner = document.createElement("th");
    corner.className = "corner";
    corner.dataset.corner = "1";
    letterRow.appendChild(corner);
    for (let c = 0; c < cols; c++) {
      const th = document.createElement("th");
      th.className = "letters";
      th.dataset.col = c;
      th.textContent = colLetter(c);
      const handle = document.createElement("div");
      handle.className = "col-resize";
      handle.dataset.resize = c;
      th.appendChild(handle);
      this.colheads.push(th);
      letterRow.appendChild(th);
    }
    thead.appendChild(letterRow);

    n.headers.forEach((row, hi) => {
      const tr = document.createElement("tr");
      const cornerCell = document.createElement("th");
      cornerCell.className = "corner";
      cornerCell.style.top = `${(hi + 1) * topOffset}px`;
      tr.appendChild(cornerCell);
      const cells = [];
      row.forEach((text, ci) => {
        const th = document.createElement("th");
        th.className = "hdr";
        th.dataset.header = `${hi}:${ci}`;
        if (this.isReadonlyCol(ci)) th.classList.add("readonly");
        if (hi === 1 && !this.isReadonlyCol(ci)) th.classList.add("export");
        if (hi === 2 && !this.isReadonlyCol(ci)) th.classList.add("type");
        th.textContent = text;
        th.style.top = `${(hi + 1) * topOffset}px`;
        cells.push(th);
        tr.appendChild(th);
      });
      this.headerCells.push(cells);
      thead.appendChild(tr);
    });
    if (n.headers[0]) this.colheads.forEach((th, c) => (th.style.top = "0px"));
    this.table.appendChild(thead);

    // 数据
    const tbody = document.createElement("tbody");
    for (let r = 0; r < this.rows; r++) {
      const tr = document.createElement("tr");
      const rowHead = document.createElement("th");
      rowHead.className = "rownum";
      rowHead.dataset.row = r;
      rowHead.textContent = r + 1;
      this.rownums.push(rowHead);
      tr.appendChild(rowHead);

      const rowCells = [];
      for (let c = 0; c < cols; c++) {
        const td = document.createElement("td");
        td.dataset.cell = `${r}:${c}`;
        if (this.isReadonlyCol(c)) td.classList.add("readonly");
        if (this.isNumericCol(c)) td.classList.add("num");
        this.paintCell(td, r, c);
        rowCells.push(td);
        tr.appendChild(td);
      }
      this.tds.push(rowCells);
      tbody.appendChild(tr);
    }
    this.table.appendChild(tbody);
  }

  paintCell(td, r, c) {
    const text = this.displayCellText(r, c);
    const kind = this.node.kinds[c] || "text";
    const arrow = ["ref", "export", "type"].includes(kind) && text !== "";
    td.innerHTML = "";
    td.appendChild(document.createTextNode(text));
    if (arrow) {
      const span = document.createElement("span");
      span.className = "arrow";
      span.textContent = "▼";
      td.appendChild(span);
    }
    td.classList.toggle("has-error", this.errorSet?.has(`${r}:${c}`) || false);
  }

  setColWidth(c, w) {
    this.colWidths[c] = Math.max(40, Math.round(w));
    this.colEls[c].style.width = this.colWidths[c] + "px";
    if (!this.autoWidth) this.cacheWidths();
  }

  cacheWidths() {
    if (!this.nodeKey || !this.colWidths.length) return;
    this.widthsByKey.set(this.nodeKey, this.colWidths.slice());
    this.emitViewChange();
  }

  /** 当前列宽（列名 → px）交给 main.js 持久化到 `.data/user-data.json`。 */
  emitViewChange() {
    if (!this.node) return;
    const names = this.node.headers?.[this.node.headers.length - 1] || [];
    const widths = {};
    for (let c = 0; c < this.cols; c++) {
      if (names[c]) widths[names[c]] = this.colWidths[c];
    }
    this.onViewChange({ auto: this.autoWidth, widths });
  }

  autoFitAll(silent) {
    if (!this.node) return;
    for (let c = 0; c < this.cols; c++) this.colWidths[c] = this.autoFitWidth(c);
    if (this.colEls) this.colEls.forEach((col, c) => (col.style.width = this.colWidths[c] + "px"));
    if (!this.autoWidth) this.cacheWidths();
    if (!silent) this.onLog(0, "列宽已自适应");
  }

  autoFitWidth(c) {
    let max = 4;
    for (const row of this.node.headers) max = Math.max(max, (row[c] || "").length);
    for (const row of this.node.rows) max = Math.max(max, (row[c] || "").length);
    return clamp(Math.round(max * 7.5 + 28), 60, 320);
  }

  setAutoWidth(on) {
    this.autoWidth = on;
    if (on) this.autoFitAll(true);
    // 自动模式下也把当前手动宽度一起存下去（取消勾选时可恢复）。
    this.cacheWidths();
  }

  // ── 选择 ────────────────────────────────────────────────
  bounds() {
    if (this.kind === "none" || !this.node) return null;
    if (this.kind === "all") return { r0: 0, r1: this.rows - 1, c0: 0, c1: this.cols - 1 };
    if (this.kind === "row") {
      const r0 = Math.min(this.anchor.r, this.focus.r);
      const r1 = Math.max(this.anchor.r, this.focus.r);
      return { r0, r1, c0: 0, c1: this.cols - 1 };
    }
    if (this.kind === "col") {
      const c0 = Math.min(this.anchor.c, this.focus.c);
      const c1 = Math.max(this.anchor.c, this.focus.c);
      return { r0: 0, r1: this.rows - 1, c0, c1 };
    }
    return {
      r0: Math.min(this.anchor.r, this.focus.r),
      r1: Math.max(this.anchor.r, this.focus.r),
      c0: Math.min(this.anchor.c, this.focus.c),
      c1: Math.max(this.anchor.c, this.focus.c),
    };
  }

  selectCell(r, c, extend) {
    if (!this.node) return;
    r = clamp(r, 0, this.rows - 1);
    c = clamp(c, 0, this.cols - 1);
    if (extend && this.kind !== "none" && this.kind !== "all") {
      this.focus = { r, c };
      this.kind = this.anchor.r === r && this.anchor.c === c ? "cell" : "range";
    } else {
      this.anchor = { r, c };
      this.focus = { r, c };
      this.kind = "cell";
    }
    this.applySelection();
    this.scrollIntoView();
    this.notify();
  }

  selectRow(r) {
    if (!this.node) return;
    this.kind = "row";
    this.focus = { r: clamp(r, 0, this.rows - 1), c: this.focus.c };
    this.anchor = { ...this.focus };
    this.applySelection();
    this.notify();
  }

  extendRowTo(r) {
    if (!this.node || this.kind !== "row") return;
    this.focus = { r: clamp(r, 0, this.rows - 1), c: this.focus.c };
    this.applySelection();
    this.notify();
  }

  selectCol(c) {
    if (!this.node) return;
    this.kind = "col";
    this.focus = { r: this.focus.r, c: clamp(c, 0, this.cols - 1) };
    this.anchor = { ...this.focus };
    this.applySelection();
    this.notify();
  }

  extendColTo(c) {
    if (!this.node || this.kind !== "col") return;
    this.focus = { r: this.focus.r, c: clamp(c, 0, this.cols - 1) };
    this.applySelection();
    this.notify();
  }

  selectAll() {
    if (!this.node) return;
    this.kind = "all";
    this.focus = { r: 0, c: 0 };
    this.anchor = { r: 0, c: 0 };
    this.applySelection();
    this.notify();
  }

  applySelection() {
    const next = this.bounds();
    const prev = this._lastBounds;
    const paint = (b) => {
      if (!b) return;
      for (let r = b.r0; r <= b.r1; r++) {
        for (let c = b.c0; c <= b.c1; c++) this.paintSelectionCell(r, c);
      }
    };
    paint(prev);
    paint(next);
    if (!prev && !next) {
      // 首次或清空：整表刷一遍，保证没有残留
      for (let r = 0; r < this.rows; r++) for (let c = 0; c < this.cols; c++) this.paintSelectionCell(r, c);
    }
    this._lastBounds = next;

    const rowIn = (r) => next && r >= next.r0 && r <= next.r1;
    const colIn = (c) => next && c >= next.c0 && c <= next.c1;
    this.rownums.forEach((th, r) => {
      th.classList.toggle("selected", this.kind === "row" && this.focus.r === r);
      th.classList.toggle("in-range", this.kind !== "row" && !!next && rowIn(r));
    });
    this.colheads.forEach((th, c) => {
      th.classList.toggle("col-selected", this.kind === "col" && this.focus.c === c);
      th.classList.toggle("in-range", this.kind !== "col" && !!next && colIn(c));
    });
  }

  paintSelectionCell(r, c) {
    const td = this.tds[r]?.[c];
    if (!td) return;
    const b = this.bounds();
    const inB = !!b && r >= b.r0 && r <= b.r1 && c >= b.c0 && c <= b.c1;
    const isFocus = this.focus.r === r && this.focus.c === c;
    td.classList.toggle("cell-selected", inB && isFocus);
    td.classList.toggle("in-range", inB && !isFocus);
  }

  selectionBounds() {
    return this.bounds();
  }

  /** 菜单触发的复制/剪切：写入内部剪贴板，并尽力同步系统剪贴板。 */
  copySelection(cut) {
    const text = this.selectionTSV();
    if (!text) return;
    this._clip = text;
    if (navigator.clipboard?.writeText) navigator.clipboard.writeText(text).catch(() => {});
    if (cut) this.clearSelection();
  }

  pasteClipboard() {
    if (this._clip) this.pasteTSV(this._clip);
  }

  selectionInfo() {    if (this.kind === "none") return "";
    if (this.kind === "all") return `整表 ${this.rows}行×${this.cols}列`;
    if (this.kind === "row") return `第 ${this.focus.r + 1} 行`;
    if (this.kind === "col") return `${colLetter(this.focus.c)} 列`;
    if (this.kind === "cell") return `${colLetter(this.focus.c)}${this.focus.r + 1}`;
    const b = this.bounds();
    const rows = b.r1 - b.r0 + 1;
    const cols = b.c1 - b.c0 + 1;
    return (
      `${colLetter(b.c0)}${b.r0 + 1}:${colLetter(b.c1)}${b.r1 + 1} ` +
      `(${rows}行×${cols}列，共${rows * cols}格)`
    );
  }

  activeCell() {
    if (this.kind === "none") return null;
    return { r: this.focus.r, c: this.focus.c };
  }

  notify() {
    this.onSelection({
      kind: this.kind,
      info: this.selectionInfo(),
      coord: this.kind === "none" ? "" : `${colLetter(this.focus.c)}${this.focus.r + 1}`,
      value: this.kind === "cell" ? this.cellText(this.focus.r, this.focus.c) : "",
      editable:
        this.kind === "cell" &&
        !this.isReadonlyCol(this.focus.c) &&
        !this.isPickerCol(this.focus.c),
      hover: this.hover ? `${colLetter(this.hover.c)}${this.hover.r + 1}` : "",
    });
  }

  scrollIntoView() {
    const td = this.tds[this.focus.r]?.[this.focus.c];
    if (!td) return;
    const view = this.scroll;
    const cell = td.getBoundingClientRect();
    const box = view.getBoundingClientRect();
    const headH = 24 * (1 + (this.node?.headers.length || 1));
    if (cell.bottom > box.bottom) view.scrollTop += cell.bottom - box.bottom;
    else if (cell.top < box.top + headH) view.scrollTop -= box.top + headH - cell.top;
    if (cell.right > box.right) view.scrollLeft += cell.right - box.right;
    else if (cell.left < box.left + 32) view.scrollLeft -= box.left + 32 - cell.left;
  }

  /** 把某一列滚进可视区。插入列后调用，否则新列会在右边界外、看起来像没插进去。 */
  revealColumn(c) {
    const th = this.colheads[clamp(c, 0, Math.max(0, this.cols - 1))];
    if (!th) return;
    const view = this.scroll;
    const cell = th.getBoundingClientRect();
    const box = view.getBoundingClientRect();
    if (cell.right > box.right) view.scrollLeft += cell.right - box.right + 8;
    else if (cell.left < box.left + 32) view.scrollLeft -= box.left + 32 - cell.left;
  }

  // ── 编辑 ────────────────────────────────────────────────
  beginCellEdit(r, c, initial) {
    if (this.locked) return;
    if (!this.node || this.isReadonlyCol(c)) return;
    const kind = this.node.kinds[c] || "text";
    // 引用列：单值用选择器；List<@X>/Set<@X> 用多选。
    if (kind === "ref" && this.node.refCols?.[c]) {
      this.openRefPicker(r, c, this.node.refCols[c], !!this.node.refList?.[c]);
      return;
    }
    // Constant 的 type 列：打开完整的类型选择对话框。
    if (kind === "type") {
      this.openTypeSelector({ kind: "cell", r, c, current: this.cellText(r, c) });
      return;
    }
    // Constant 的 export 列用候选输入；Table 的同类语义在表头，不在这里。
    const inputKind = kind === "export" ? "export" : "text";
    this.beginEdit({ type: "cell", r, c }, initial, inputKind);
  }

  /** 类型选择对话框由 main.js 弹；这里只负责把结果写回表头或单元格。 */
  async openTypeSelector(target) {
    this.commitEdit(true);
    let value = null;
    try {
      value = await this.onTypeSelector(target);
    } catch (e) {
      this.onLog(2, String(e));
      return;
    }
    if (value == null) return;
    if (target.kind === "header") {
      this.onCommit({ type: "header", hi: target.hi, ci: target.ci }, value);
    } else {
      this.setCellValue(target.r, target.c, value);
    }
  }

  beginHeaderEdit(hi, ci, initial) {
    if (this.locked) return;
    if (!this.node || !this.node.editableHeaders) return;
    const kind = this.node.headerKinds?.[hi]?.[ci] || "text";
    if (kind === "readonly") return;
    this.beginEdit({ type: "header", hi, ci }, initial, kind);
  }

  beginEdit(target, initial, inputKind = "text") {
    this.commitEdit(true);
    const el =
      target.type === "cell"
        ? this.tds[target.r][target.c]
        : this.headerCells[target.hi][target.ci];
    if (!el) return;
    const value =
      target.type === "cell" ? this.cellText(target.r, target.c) : this.node.headers[target.hi][target.ci];
    this.editing = { ...target, el, original: value };

    let input;
    if (inputKind === "export") {
      input = document.createElement("select");
      ["前后端", "客户端", "服务器", "不导出"].forEach((opt) => {
        const o = document.createElement("option");
        o.value = opt;
        o.textContent = opt;
        input.appendChild(o);
      });
      input.value = value || "前后端";
      input.classList.add("cell-edit", "select-edit");
    } else if (inputKind === "type") {
      input = document.createElement("select");
      input.innerHTML = typeOptionsHtml(value, this.node.refTargets || []);
      input.classList.add("cell-edit", "select-edit");
    } else {
      input = document.createElement("input");
      input.className = "cell-edit";
      input.value = initial !== undefined ? String(initial) : value;
      if (inputKind === "type") {
        input.setAttribute("list", "type-options");
      }
    }
    el.innerHTML = "";
    el.appendChild(input);
    input.focus();
    if (input.select) input.select();

    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") this.commitEdit(true, "down");
      else if (e.key === "Escape") this.cancelEdit();
      else if (e.key === "Tab") {
        e.preventDefault();
        this.commitEdit(true, e.shiftKey ? "left" : "right");
      }
    });
    input.addEventListener("change", () => {
      if (inputKind === "export") {
        this.commitEdit(true, "right");
        return;
      }
      if (inputKind === "type") {
        if (input.value === "__custom__") {
          // 切回文本编辑：先撤销当前选择器（不提交 __custom__），再用原值开输入框。
          this.cancelEdit();
          this.beginEdit(target, value, "text");
          return;
        }
        this.commitEdit(true, "right");
      }
    });
    input.addEventListener("blur", () => {
      if (this.autoCommitOnBlur) this.commitEdit(true);
      else this.cancelEdit();
    });
  }

  commitEdit(save, move) {
    if (!this.editing) return;
    const ed = this.editing;
    const input = ed.el.querySelector("input,select");
    const value = input ? input.value : ed.original;
    this.editing = null;

    if (save && value !== ed.original) {
      if (ed.type === "cell") {
        this.node.rows[ed.r][ed.c] = value;
      } else {
        this.node.headers[ed.hi][ed.ci] = value;
      }
      this.onCommit(ed, value);
    }

    if (ed.type === "cell") this.paintCell(this.tds[ed.r][ed.c], ed.r, ed.c);
    else this.paintHeader(this.headerCells[ed.hi][ed.ci], ed.hi, ed.ci);

    if (move && ed.type === "cell") {
      const dr = move === "down" ? 1 : move === "up" ? -1 : 0;
      const dc = move === "right" ? 1 : move === "left" ? -1 : 0;
      this.selectCell(ed.r + dr, ed.c + dc, false);
    }
    this.applySelection();
  }

  cancelEdit() {
    if (!this.editing) return;
    const ed = this.editing;
    this.editing = null;
    if (ed.type === "cell") this.paintCell(this.tds[ed.r][ed.c], ed.r, ed.c);
    else this.paintHeader(this.headerCells[ed.hi][ed.ci], ed.hi, ed.ci);
    this.applySelection();
  }

  paintHeader(th, hi, ci) {
    const text = this.node.headers[hi][ci];
    th.textContent = text;
    if ((hi === 1 || hi === 2) && !this.isReadonlyCol(ci)) {
      const span = document.createElement("span");
      span.className = "arrow";
      span.textContent = "▼";
      th.appendChild(span);
    }
  }

  setActiveValue(value) {
    if (this.kind !== "cell" && this.kind !== "range") return;
    if (this.setCellText(this.focus.r, this.focus.c, value)) {
      this.paintCell(this.tds[this.focus.r][this.focus.c], this.focus.r, this.focus.c);
      this.onCommit({ type: "cell", r: this.focus.r, c: this.focus.c }, value);
      this.notify();
    }
  }

  // ── 剪贴板 ──────────────────────────────────────────────
  selectionTSV() {
    const b = this.bounds();
    if (!b) return "";
    const lines = [];
    for (let r = b.r0; r <= b.r1; r++) {
      const row = [];
      for (let c = b.c0; c <= b.c1; c++) row.push(this.cellText(r, c));
      lines.push(row.join("\t"));
    }
    return lines.join("\n");
  }

  clearSelection() {
    const b = this.bounds();
    if (!b) return;
    let changed = false;
    const cells = [];
    for (let r = b.r0; r <= b.r1; r++) {
      for (let c = b.c0; c <= b.c1; c++) {
        cells.push([r, c]);
        if (this.setCellText(r, c, "")) {
          changed = true;
          this.paintCell(this.tds[r][c], r, c);
        }
      }
    }
    if (changed) this.onCommit({ type: "clear", cells }, "");
  }

  pasteTSV(text) {
    const b = this.bounds();
    if (!b) return;
    const rows = text.replace(/\r/g, "").split("\n");
    if (rows.length && rows[rows.length - 1] === "") rows.pop();
    const grid = rows.map((line) => line.split("\t"));
    let changed = false;
    grid.forEach((line, dr) => {
      const r = b.r0 + dr;
      if (r >= this.rows) return;
      line.forEach((value, dc) => {
        const c = b.c0 + dc;
        if (c >= this.cols) return;
        if (this.setCellText(r, c, value)) {
          changed = true;
          this.paintCell(this.tds[r][c], r, c);
        }
      });
    });
    if (changed) this.onCommit({ type: "paste", r: b.r0, c: b.c0, grid }, "");
  }

  // ── 事件 ────────────────────────────────────────────────
  _bind() {
    const scroll = this.scroll;

    scroll.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      scroll.focus({ preventScroll: true });

      const resize = e.target.closest("[data-resize]");
      if (resize) {
        this.startResize(+resize.dataset.resize, e);
        return;
      }
      if (e.target.closest("[data-corner]")) {
        this.selectAll();
        return;
      }
      const rownum = e.target.closest("th[data-row]");
      if (rownum) {
        this.dragAxis = "row";
        this.selectRow(+rownum.dataset.row);
        return;
      }
      const colhead = e.target.closest("th[data-col]");
      if (colhead) {
        this.dragAxis = "col";
        this.selectCol(+colhead.dataset.col);
        return;
      }
      const header = e.target.closest("th[data-header]");
      if (header) {
        // 导出 / 类型是选择器：单击即展开（与 Slint 默认 picker-trigger=header-single 一致）。
        const [hi, ci] = header.dataset.header.split(":").map(Number);
        const kind = this.node?.headerKinds?.[hi]?.[ci] || "text";
        if ((kind === "export" || kind === "type") && (this.node?.pickerHeaderSingle ?? true)) {
          this.openHeaderPicker(hi, ci, kind);
        }
        return;
      }
      const td = e.target.closest("td[data-cell]");
      if (!td) return;
      const [r, c] = td.dataset.cell.split(":").map(Number);
      this.commitEdit(true);
      this.dragging = true;
      this.selectCell(r, c, e.shiftKey);
      const kind = this.node?.kinds?.[c];
      if ((kind === "type" || kind === "export") && this.node?.pickerDataSingle) {
        this.beginCellEdit(r, c);
      }
    });

    document.addEventListener("pointermove", (e) => {
      if (this.dragAxis) {
        const el = document.elementFromPoint(e.clientX, e.clientY);
        if (this.dragAxis === "row") {
          const th = el?.closest?.("th[data-row]");
          if (th) this.extendRowTo(+th.dataset.row);
        } else {
          const th = el?.closest?.("th[data-col]");
          if (th) this.extendColTo(+th.dataset.col);
        }
        return;
      }
      if (this.dragging) {
        this._dragPoint = { x: e.clientX, y: e.clientY };
        this.dragToPoint(e.clientX, e.clientY);
        this.autoScrollTick();
        return;
      }
      const td = e.target.closest?.("td[data-cell]");
      if (td) {
        const [r, c] = td.dataset.cell.split(":").map(Number);
        if (!this.hover || this.hover.r !== r || this.hover.c !== c) {
          this.hover = { r, c };
          this.notify();
        }
      }
    });

    document.addEventListener("pointerup", () => {
      this.dragging = false;
      this.dragAxis = null;
      cancelAnimationFrame(this._raf);
      this._raf = 0;
    });

    scroll.addEventListener("dblclick", (e) => {
      const td = e.target.closest("td[data-cell]");
      if (td) {
        const [r, c] = td.dataset.cell.split(":").map(Number);
        const kind = this.node?.kinds?.[c];
        if ((kind === "type" || kind === "export") && this.node?.pickerDataSingle) return;
        this.beginCellEdit(r, c);
        return;
      }
      const header = e.target.closest("th[data-header]");
      if (header) {
        const [hi, ci] = header.dataset.header.split(":").map(Number);
        const kind = this.node?.headerKinds?.[hi]?.[ci] || "text";
        if (kind === "export" || kind === "type") {
          this.openHeaderPicker(hi, ci, kind);
          return;
        }
        this.beginHeaderEdit(hi, ci);
        return;
      }
      const resize = e.target.closest("[data-resize]");
      if (resize) {
        const c = +resize.dataset.resize;
        this.autoWidth = false;
        this.setColWidth(c, this.autoFitWidth(c));
        this.onLog(0, `${colLetter(c)} 列宽已自适应`);
      }
    });

    scroll.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const at = { x: e.clientX, y: e.clientY };
      const td = e.target.closest("td[data-cell]");
      if (td) {
        const [r, c] = td.dataset.cell.split(":").map(Number);
        const b = this.bounds();
        const inside = !!b && r >= b.r0 && r <= b.r1 && c >= b.c0 && c <= b.c1;
        if (!inside) this.selectCell(r, c, false);
        this.onContextMenu({ shape: "cell", ...at });
        return;
      }
      const rownum = e.target.closest("th[data-row]");
      if (rownum) {
        this.selectRow(+rownum.dataset.row);
        this.onContextMenu({ shape: "row", ...at });
        return;
      }
      const colhead = e.target.closest("th[data-col]");
      if (colhead) {
        this.selectCol(+colhead.dataset.col);
        this.onContextMenu({ shape: "col", ...at });
        return;
      }
      this.onContextMenu({ shape: "blank", ...at });
    });

    // 表格内不允许浏览器原生文本选择：拖动选区应该是格子选区，
    // 而不是把表头 / 只读文字一起选进去，导致 Ctrl+C 复制到页面文本。
    scroll.addEventListener("scroll", () => { this.closeRefPicker(); this.closeHeaderPicker(); });
    scroll.addEventListener("selectstart", (e) => e.preventDefault());
    scroll.addEventListener("dragstart", (e) => e.preventDefault());

    scroll.addEventListener("keydown", (e) => this.onKeyDown(e));

    scroll.addEventListener("copy", (e) => {
      if (this.editing) return;
      const text = this.selectionTSV();
      if (!text) return;
      e.clipboardData.setData("text/plain", text);
      e.preventDefault();
      document.getSelection()?.removeAllRanges();
    });
    scroll.addEventListener("cut", (e) => {
      if (this.editing) return;
      const text = this.selectionTSV();
      if (!text) return;
      e.clipboardData.setData("text/plain", text);
      e.preventDefault();
      this.clearSelection();
    });
    scroll.addEventListener("paste", (e) => {
      if (this.editing) return;
      const text = e.clipboardData.getData("text/plain");
      if (!text) return;
      e.preventDefault();
      this.pasteTSV(text);
    });
  }

  onKeyDown(e) {
    if (this.editing) return;
    const mod = e.ctrlKey || e.metaKey;

    if (mod && (e.key === "c" || e.key === "x")) {
      // 交给浏览器 copy/cut 事件
      return;
    }
    if (mod && e.key === "v") {
      // 交给浏览器 paste 事件
      return;
    }
    if (mod && e.key === "a") {
      e.preventDefault();
      this.selectAll();
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      this.clearSelection();
      return;
    }
    if (e.key === "F2") {
      e.preventDefault();
      this.beginCellEdit(this.focus.r, this.focus.c);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (this.kind === "none") this.selectCell(0, 0, false);
      else if (e.altKey) this.beginCellEdit(this.focus.r, this.focus.c);
      else this.selectCell(this.focus.r + (e.shiftKey ? -1 : 1), this.focus.c, false);
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const dc = e.shiftKey ? -1 : 1;
      this.selectCell(this.focus.r, this.focus.c + dc, false);
      return;
    }
    if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) {
      e.preventDefault();
      if (this.kind === "none") {
        this.selectCell(0, 0, false);
        return;
      }
      const dr = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
      const dc = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
      if (mod && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        this.selectCell(e.key === "ArrowDown" ? this.rows - 1 : 0, this.focus.c, false);
      } else if (mod && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        this.selectCell(this.focus.r, e.key === "ArrowRight" ? this.cols - 1 : 0, false);
      } else {
        this.selectCell(this.focus.r + dr, this.focus.c + dc, e.shiftKey);
      }
      return;
    }
    if (e.key === "Home") {
      e.preventDefault();
      this.selectCell(e.ctrlKey ? 0 : this.focus.r, 0, false);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      this.selectCell(e.ctrlKey ? this.rows - 1 : this.focus.r, this.cols - 1, false);
      return;
    }
    if (e.key === "PageDown" || e.key === "PageUp") {
      e.preventDefault();
      const step = Math.max(1, Math.floor(this.scroll.clientHeight / (this.rowH || 24)) - 2);
      this.selectCell(this.focus.r + (e.key === "PageDown" ? step : -step), this.focus.c, false);
      return;
    }
    // 直接打字进入编辑
    if (e.key.length === 1 && !mod && !e.altKey) {
      e.preventDefault();
      this.beginCellEdit(this.focus.r, this.focus.c, e.key);
    }
  }

  dragToPoint(x, y) {
    const el = document.elementFromPoint(x, y);
    const td = el?.closest?.("td[data-cell]");
    if (!td) return;
    const [r, c] = td.dataset.cell.split(":").map(Number);
    if (r !== this.focus.r || c !== this.focus.c) {
      this.focus = { r, c };
      this.kind = this.anchor.r === r && this.anchor.c === c ? "cell" : "range";
      this.applySelection();
      this.notify();
    }
  }

  autoScrollTick() {
    if (!this.dragging) return;
    const p = this._dragPoint;
    if (!p) return;
    const box = this.scroll.getBoundingClientRect();
    const edge = 28;
    let dx = 0;
    let dy = 0;
    if (p.x < box.left + edge) dx = -14;
    else if (p.x > box.right - edge) dx = 14;
    if (p.y < box.top + edge) dy = -14;
    else if (p.y > box.bottom - edge) dy = 14;
    if (dx || dy) {
      this.scroll.scrollLeft += dx;
      this.scroll.scrollTop += dy;
      this.dragToPoint(p.x, p.y);
      this._raf = requestAnimationFrame(() => this.autoScrollTick());
    }
  }

  startResize(c, e) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = this.colWidths[c];
    this.autoWidth = false;
    const move = (ev) => this.setColWidth(c, startW + ev.clientX - startX);
    const up = () => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
      this.onLog(0, `${colLetter(c)} 列宽 ${this.colWidths[c]}px`);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", up);
  }
}

window.SheetGrid = SheetGrid;
