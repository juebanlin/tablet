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
/** 最后一条数据列之后的空白宽度：内容刚好铺满视口时也能横向滚动。 */
const SPACER_W = 160;

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
    // 大表虚拟滚动：只把可视区附近的行放进 DOM，其余用上下 spacer 撑高度。
    this.virtual = false;
    this.visibleStart = 0;
    this.visibleEnd = 0;
    this.virtualBuffer = 10;
    this._vraf = 0;
    this.tbody = null;
    this.headerH = 0;

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
    // 拖拽列宽期间如果恰好收到一次快照刷新（大表校验慢时常见），快照里的旧列宽
    // 会把手动调整覆盖掉；这里保留拖拽中的本地列宽，松手后由 save_grid_view 落盘。
    const keepWidths = this._resizing && sameNode
      ? { auto: this.autoWidth, widths: this.colWidths.slice() }
      : null;
    const keep =
      opts.keepSelection && sameNode
        ? { anchor: { ...this.anchor }, focus: { ...this.focus }, kind: this.kind }
        : null;
    // 同一节点的快照刷新（粘贴 / 插入 / 删除 / 撤销等走完整 Snapshot）不能把视口弹回顶部：
    // render() 重建前会清空表格，布局会把 scrollTop 钳到 0，所以先存后恢复。
    const keepScroll = sameNode ? { top: this.scroll.scrollTop, left: this.scroll.scrollLeft } : null;
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
    if (keepWidths && keepWidths.widths.length === this.colWidths.length) {
      this.autoWidth = keepWidths.auto;
      this.colWidths = keepWidths.widths;
    }
    this.render();

    // 恢复同一节点的滚动位置；render() 是从顶部重新建窗口的，需要按恢复后的位置再换一次窗口。
    if (keepScroll) {
      this.scroll.scrollTop = keepScroll.top;
      this.scroll.scrollLeft = keepScroll.left;
      if (this.virtual) this.renderVirtualRows(true);
    }

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

  /** 局部刷新展示值（引用列显示名字）；只改 displayRows，不动存盘原值。 */
  setDisplayCellText(r, c, text) {
    if (!this.node) return false;
    const rows = this.node.displayRows || (this.node.displayRows = []);
    while (rows.length <= r) rows.push([]);
    const row = rows[r] || (rows[r] = []);
    if (row[c] === text) return false;
    row[c] = text;
    const td = this.tds[r]?.[c];
    if (td) this.paintCell(td, r, c);
    return true;
  }

  setCellValue(r, c, value) {
    if (this.setCellText(r, c, value)) {
      const td = this.tds[r]?.[c];
      if (td) this.paintCell(td, r, c);
      this.onCommit({ type: "cell", r, c }, value);
      this.notify();
    }
  }

  /**
   * 引用选择器（对齐 Slint 版）：id + 辅助列的表格式列表。
   * 表可切换「智能 / 全部」列展示；单值单击选中、双击/确定提交；
   * 列表引用多选后按该列类型的分隔符拼接（List 取项目 `list`、Set 取 `set`，默认 `;`）。
   */
  async openRefPicker(r, c, target, multi = false, sepArg = ";") {
    this.closeRefPicker();
    const td = this.tds[r]?.[c];
    if (!td) return;
    const rect = td.getBoundingClientRect();
    const raw = this.cellText(r, c);
    // 分隔符为空说明项目配置异常；退回默认值，避免 split("") 把字符逐个拆开。
    const sep = sepArg || ";";
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
    // 菜单宽度按内容自适应，不跟随列宽（宽列会把菜单拉得很大）。
    popup.style.width = "max-content";
    popup.style.minWidth = "120px";
    popup.style.maxWidth = "min(280px, calc(100vw - 16px))";
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
    this.tbody = null;
    this.virtual = false;
    this.visibleStart = 0;
    this.visibleEnd = 0;
    if (!this.node) {
      this.table.hidden = true;
      return;
    }
    this.table.hidden = false;

    const n = this.node;
    const cols = this.cols;

    // 列宽
    const colgroup = document.createElement("colgroup");
    // 行号宽度按最大行号的位数自适应（10 万行需要 6 位数字）。
    const gutterW = Math.max(32, 14 + String(this.rows).length * 8);
    this.gutterW = gutterW;
    this.table.style.setProperty("--gutter-w", `${gutterW}px`);
    const gutter = document.createElement("col");
    gutter.style.width = `${gutterW}px`;
    colgroup.appendChild(gutter);
    const colEls = [];
    for (let c = 0; c < cols; c++) {
      const col = document.createElement("col");
      col.style.width = this.colWidths[c] + "px";
      colEls.push(col);
      colgroup.appendChild(col);
    }
    // 末尾空白列：给最后一条数据之后留出一条空白，像 Excel 一样可以继续横向滚动。
    const spacerCol = document.createElement("col");
    spacerCol.style.width = `${SPACER_W}px`;
    colgroup.appendChild(spacerCol);
    this.colEls = colEls;
    this.table.appendChild(colgroup);
    // 显式表格宽度 = 各列 + 行号 + 末尾空白。否则 `table-layout: fixed` 的表格
    // 宽度 auto 会填满视口，列一多就按比例压缩列宽，永远不会出现横向滚动条。
    this.syncTableWidth();

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
    letterRow.appendChild(this.makeSpacerCell("th"));
    thead.appendChild(letterRow);

    n.headers.forEach((row, hi) => {
      const tr = document.createElement("tr");
      const cornerCell = document.createElement("th");
      cornerCell.className = "corner";
      tr.appendChild(cornerCell);
      const cells = [];
      row.forEach((text, ci) => {
        const th = document.createElement("th");
        th.className = "hdr";
        th.dataset.header = `${hi}:${ci}`;
        if (this.node.kinds?.[ci] === "aux") th.classList.add("aux");
        if (this.isReadonlyCol(ci)) th.classList.add("readonly");
        if (hi === 1 && !this.isReadonlyCol(ci)) th.classList.add("export");
        if (hi === 2 && !this.isReadonlyCol(ci)) th.classList.add("type");
        th.textContent = text;
        cells.push(th);
        tr.appendChild(th);
      });
      tr.appendChild(this.makeSpacerCell("th"));
      this.headerCells.push(cells);
      thead.appendChild(tr);
    });
    this.table.appendChild(thead);
    // 表头行高固定（CSS `--row-h`，nowrap），直接按行数算。
    // 不能在这里 getBoundingClientRect()：那会在 tbody 还空着时强制布局，
    // 浏览器随即把 scrollTop 钳到 0，同节点刷新（粘贴/撤销等）就会跳回第一行。
    this.headerH = rowH * (1 + n.headers.length);

    // 数据
    const tbody = document.createElement("tbody");
    this.tbody = tbody;
    if (this.rows > 500) {
      // 大表：只渲染可视窗口；滚动时换窗口。
      this.virtual = true;
      this.tds = new Array(this.rows);
      this.rownums = new Array(this.rows);
      this.table.appendChild(tbody);
      this.renderVirtualRows(true);
      return;
    }
    for (let r = 0; r < this.rows; r++) tbody.appendChild(this.makeRow(r));
    // 与虚拟模式同理：末尾补不足一行的空白，滚到底时第一行仍能贴住表头。
    const pad = (((this.scroll.clientHeight - this.headerH) % rowH) + rowH) % rowH;
    if (pad > 0) tbody.appendChild(this.virtualGapRow(pad));
    this.table.appendChild(tbody);
  }

  /** 建一行（绝对行号 r），并把 td/行号登记到 this.tds / this.rownums。 */
  makeRow(r) {
    const tr = document.createElement("tr");
    // 斑马纹按绝对行号，否则虚拟滚动后条纹会跟着窗口漂移。
    if (r % 2 === 1) tr.classList.add("alt");
    const rowHead = document.createElement("th");
    rowHead.className = "rownum";
    rowHead.dataset.row = r;
    rowHead.textContent = r + 1;
    this.rownums[r] = rowHead;
    tr.appendChild(rowHead);

    const rowCells = [];
    const cols = this.cols;
    for (let c = 0; c < cols; c++) {
      const td = document.createElement("td");
      td.dataset.cell = `${r}:${c}`;
      if (this.node.kinds?.[c] === "aux") td.classList.add("aux");
      if (this.isReadonlyCol(c)) td.classList.add("readonly");
      if (this.isNumericCol(c)) td.classList.add("num");
      this.paintCell(td, r, c);
      rowCells.push(td);
      tr.appendChild(td);
    }
    tr.appendChild(this.makeSpacerCell("td"));
    this.tds[r] = rowCells;
    return tr;
  }

  /** 表格末尾的空白单元格：只占位，不参与选中 / 复制 / 编辑。 */
  makeSpacerCell(tag) {
    const cell = document.createElement(tag);
    cell.className = "spacer";
    return cell;
  }

  virtualGapRow(height) {
    const tr = document.createElement("tr");
    tr.className = "virtual-gap";
    const td = document.createElement("td");
    td.colSpan = this.cols + 2;
    td.style.height = `${height}px`;
    td.style.padding = "0";
    td.style.border = "0";
    td.style.lineHeight = "0";
    tr.appendChild(td);
    return tr;
  }

  /** 重算虚拟窗口并重建 tbody（只在窗口变化时）。 */
  renderVirtualRows(force = false) {
    if (!this.virtual || !this.node || !this.tbody) return;
    const view = this.scroll;
    const rowH = this.rowH || 24;
    // 表头 sticky 在滚动容器顶部（高 headerH），会盖住下方内容；tbody 用等高
    // spacer 代替未渲染的行。行 s 的视口顶 = headerH + s*rowH - scrollTop，
    // 「第一整行」从 scrollTop / rowH 起算。若从 scrollTop-headerH 起算，
    // 最上面会露出半行（数字被切掉一半），看起来像和表头重叠。
    // 留 1px 容差：浏览器会按 DPI 把 scrollTop 存成小数（如 345686.28），
    // 直接 ceil 会在刚好对齐整行时多算一行，露出整整一行空白。
    const start = clamp(
      Math.floor((view.scrollTop + 1) / rowH),
      0,
      Math.max(0, this.rows - 1),
    );
    const end = clamp(
      Math.ceil((view.scrollTop + view.clientHeight) / rowH) + this.virtualBuffer,
      start,
      this.rows,
    );
    if (!force && start === this.visibleStart && end === this.visibleEnd) return;
    if (this.editing) this.cancelEdit();

    for (let r = this.visibleStart; r < this.visibleEnd; r++) {
      if (this.tds[r]) this.tds[r] = undefined;
      if (this.rownums[r]) this.rownums[r] = undefined;
    }

    const frag = document.createDocumentFragment();
    const topH = start * rowH;
    if (topH > 0) frag.appendChild(this.virtualGapRow(topH));
    for (let r = start; r < end; r++) frag.appendChild(this.makeRow(r));
    // 末尾补一点空白，让「内容高度 - 视口高度」正好是行高的整数倍；
    // 否则滚到最底部时 max scrollTop 不在行边界上，表头下又会露出半行。
    const pad = (((view.clientHeight - (this.headerH || 0)) % rowH) + rowH) % rowH;
    const bottomH = (this.rows - end) * rowH + pad;
    if (bottomH > 0) frag.appendChild(this.virtualGapRow(bottomH));
    this.tbody.replaceChildren(frag);
    this.visibleStart = start;
    this.visibleEnd = end;
    this.applySelection();
  }

  /**
   * 把纵向滚动位置对齐到整行。
   * 行高固定，sticky 表头会盖住滚动容器顶部；滚动位置不在行边界上时，
   * 表头下沿会露出被切掉一半的行（数字/文字只剩半截），或留下一条空白带。
   * 对齐后第一行正好贴在表头下沿，滚动以整行为步长（和 Excel 一致）。
   * 触底时不强行对齐，否则最后一行会被挡在视口外。
   */
  snapScrollToRow() {
    if (!this.node || this._snapping) return;
    const view = this.scroll;
    const rowH = this.rowH || 24;
    const max = view.scrollHeight - view.clientHeight;
    if (view.scrollTop >= max - 1) return;
    const snapped = Math.round(view.scrollTop / rowH) * rowH;
    if (Math.abs(snapped - view.scrollTop) < 0.5) return;
    this._snapping = true;
    view.scrollTop = snapped;
    this._snapping = false;
  }

  /** 轻量编辑结果只更新错误 class，不重建 DOM。 */
  setErrors(errors) {
    this.errorSet = new Set((errors || []).map(([r, c]) => `${r}:${c}`));
    if (!this.node) return;
    const r0 = this.virtual ? this.visibleStart : 0;
    const r1 = this.virtual ? this.visibleEnd : this.rows;
    for (let r = r0; r < r1; r++) {
      const row = this.tds[r];
      if (!row) continue;
      for (let c = 0; c < row.length; c++) {
        if (!row[c]) continue;
        row[c].classList.toggle("has-error", this.errorSet.has(`${r}:${c}`));
      }
    }
  }

  paintCell(td, r, c) {    const text = this.displayCellText(r, c);
    td.innerHTML = "";
    td.appendChild(document.createTextNode(text));
    td.classList.toggle("has-error", this.errorSet?.has(`${r}:${c}`) || false);
  }

  setColWidth(c, w) {
    this.colWidths[c] = Math.max(40, Math.round(w));
    this.colEls[c].style.width = this.colWidths[c] + "px";
    this.syncTableWidth();
    if (!this.autoWidth) this.cacheWidths();
  }

  /** 把表格宽度钉成列宽总和，超出视口时由 .grid-scroll 出横向滚动条。 */
  syncTableWidth() {
    if (!this.node || !this.colWidths.length) return;
    const total = this.gutterW + this.colWidths.reduce((a, b) => a + b, 0) + SPACER_W;
    this.table.style.width = `${total}px`;
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
    this.syncTableWidth();
    if (!this.autoWidth) this.cacheWidths();
    if (!silent) this.onLog(0, "列宽已自适应");
  }

  autoFitWidth(c) {
    let max = 4;
    for (const row of this.node.headers) max = Math.max(max, (row[c] || "").length);
    const rows = this.node.rows;
    // 大表不扫全量：前 1000 行足以代表列宽，避免 10 万行 × 20 列逐格量长度。
    const limit = rows.length > 5000 ? Math.min(rows.length, 1000) : rows.length;
    for (let i = 0; i < limit; i++) {
      max = Math.max(max, (rows[i]?.[c] || "").length);
    }
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
      const r0 = this.virtual ? Math.max(b.r0, this.visibleStart) : b.r0;
      const r1 = this.virtual ? Math.min(b.r1, this.visibleEnd - 1) : b.r1;
      for (let r = r0; r <= r1; r++) {
        for (let c = b.c0; c <= b.c1; c++) this.paintSelectionCell(r, c);
      }
    };
    paint(prev);
    paint(next);
    if (!prev && !next) {
      // 首次或清空：整表刷一遍，保证没有残留
      if (this.virtual) {
        for (let r = this.visibleStart; r < this.visibleEnd; r++) {
          for (let c = 0; c < this.cols; c++) this.paintSelectionCell(r, c);
        }
      } else {
        for (let r = 0; r < this.rows; r++) for (let c = 0; c < this.cols; c++) this.paintSelectionCell(r, c);
      }
    }
    this._lastBounds = next;

    const rowIn = (r) => next && r >= next.r0 && r <= next.r1;
    const colIn = (c) => next && c >= next.c0 && c <= next.c1;
    this.rownums.forEach((th, r) => {
      if (!th) return;
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
    if (this.kind === "cell") {
      const base = `${colLetter(this.focus.c)}${this.focus.r + 1}`;
      return this.node?.kinds?.[this.focus.c] === "aux" ? `${base} · 普通列（不导出）` : base;
    }
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
    const view = this.scroll;
    const rowH = this.rowH || 24;
    const headerH = this.headerH || rowH * (1 + (this.node?.headers?.length || 1));
    if (this.virtual) {
      const top = headerH + this.focus.r * rowH;
      const bottom = top + rowH;
      // 目标位置按整行对齐（与 snapScrollToRow 一致）：向下取整行保证格子底边露全，
      // 向上取整行保证格子顶边不被表头盖住。
      const before = view.scrollTop;
      if (bottom > view.scrollTop + view.clientHeight) {
        view.scrollTop = Math.ceil((bottom - view.clientHeight) / rowH) * rowH;
      } else if (top < view.scrollTop + headerH) {
        view.scrollTop = Math.floor((top - headerH) / rowH) * rowH;
      }
      // 只有滚动位置变化或目标行不在当前窗口里才重建 tbody。
      // 每次点选都重建会把 mouseup 时的 td 换掉，浏览器就不派发 click/dblclick，
      // 双击编辑因此失效。
      const inWindow = this.focus.r >= this.visibleStart && this.focus.r < this.visibleEnd;
      if (view.scrollTop !== before || !inWindow) this.renderVirtualRows(true);
    }
    const td = this.tds[this.focus.r]?.[this.focus.c];
    if (!td) {
      // 大表行被虚拟化时，用列头做水平定位。
      const th = this.colheads[this.focus.c];
      if (!th) return;
      const cell = th.getBoundingClientRect();
      const box = view.getBoundingClientRect();
      if (cell.right > box.right) view.scrollLeft += cell.right - box.right;
      else if (cell.left < box.left + (this.gutterW || 32))
        view.scrollLeft -= box.left + (this.gutterW || 32) - cell.left;
      return;
    }
    const cell = td.getBoundingClientRect();
    const box = view.getBoundingClientRect();
    if (cell.bottom > box.bottom) view.scrollTop += cell.bottom - box.bottom;
    else if (cell.top < box.top + headerH) view.scrollTop -= box.top + headerH - cell.top;
    if (cell.right > box.right) view.scrollLeft += cell.right - box.right;
    else if (cell.left < box.left + (this.gutterW || 32))
      view.scrollLeft -= box.left + (this.gutterW || 32) - cell.left;
  }

  /** 把某一列滚进可视区。插入列后调用，否则新列会在右边界外、看起来像没插进去。 */
  revealColumn(c) {
    const th = this.colheads[clamp(c, 0, Math.max(0, this.cols - 1))];
    if (!th) return;
    const view = this.scroll;
    const cell = th.getBoundingClientRect();
    const box = view.getBoundingClientRect();
    if (cell.right > box.right) view.scrollLeft += cell.right - box.right + 8;
    else if (cell.left < box.left + (this.gutterW || 32))
      view.scrollLeft -= box.left + (this.gutterW || 32) - cell.left;
  }

  // ── 编辑 ────────────────────────────────────────────────
  beginCellEdit(r, c, initial) {
    if (this.locked) return;
    if (!this.node || this.isReadonlyCol(c)) return;
    const kind = this.node.kinds[c] || "text";
    // 引用列：单值用选择器；List<@X>/Set<@X> 用多选。
    if (kind === "ref" && this.node.refCols?.[c]) {
      this.openRefPicker(
        r,
        c,
        this.node.refCols[c],
        !!this.node.refList?.[c],
        this.node.refSeps?.[c] || ";",
      );
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
        ? this.tds[target.r]?.[target.c]
        : this.headerCells[target.hi]?.[target.ci];
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

    if (ed.type === "cell") {
      const td = this.tds[ed.r]?.[ed.c];
      if (td) this.paintCell(td, ed.r, ed.c);
    } else this.paintHeader(this.headerCells[ed.hi][ed.ci], ed.hi, ed.ci);

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
    if (ed.type === "cell") {
      const td = this.tds[ed.r]?.[ed.c];
      if (td) this.paintCell(td, ed.r, ed.c);
    } else this.paintHeader(this.headerCells[ed.hi][ed.ci], ed.hi, ed.ci);
    this.applySelection();
  }

  paintHeader(th, hi, ci) {
    th.textContent = this.node.headers[hi][ci];
  }

  setActiveValue(value) {
    if (this.kind !== "cell" && this.kind !== "range") return;
    if (this.setCellText(this.focus.r, this.focus.c, value)) {
      const td = this.tds[this.focus.r]?.[this.focus.c];
      if (td) this.paintCell(td, this.focus.r, this.focus.c);
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
          const td = this.tds[r]?.[c];
          if (td) this.paintCell(td, r, c);
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
          const td = this.tds[r]?.[c];
          if (td) this.paintCell(td, r, c);
        }
      });
    });
    if (changed) this.onCommit({ type: "paste", r: b.r0, c: b.c0, grid }, "");
  }

  // ── 事件 ────────────────────────────────────────────────
  _bind() {
    const scroll = this.scroll;

    scroll.addEventListener("scroll", () => {
      this.snapScrollToRow();
      if (!this.virtual || this._vraf) return;
      this._vraf = requestAnimationFrame(() => {
        this._vraf = 0;
        this.renderVirtualRows();
      });
    });

    scroll.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      // 阻止原生文本选择 / 拖拽。双击编辑依赖 dblclick，不能因为这里的 preventDefault
      // 或 tbody 重建而丢事件：点选时只有窗口真的换了才重建（见 scrollIntoView）。
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
    this._resizing = true;
    const move = (ev) => this.setColWidth(c, startW + ev.clientX - startX);
    const up = () => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
      this._resizing = false;
      this.onLog(0, `${colLetter(c)} 列宽 ${this.colWidths[c]}px`);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", up);
  }
}

window.SheetGrid = SheetGrid;
