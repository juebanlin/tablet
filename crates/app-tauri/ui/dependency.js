// 依赖分析视图：项目级引用图。
// 数据由 Rust `dependency_graph` 命令生成；前端只做分层布局、筛选与详情渲染。
// 入口：顶部「依赖图」按钮 / 项目右键菜单。

(() => {
  const $ = (id) => document.getElementById(id);
  const W = 144;
  const H = 56;
  const SAT_W = 118;
  const SAT_H = 42;
  const GAP = 20;
  const ROW_GAP = 124;

  const ROLE_LABEL = {
    base: "基表",
    business: "业务表",
    root: "无入边",
    bridge: "桥接",
    satellite: "附属",
    orphan: "未接入",
    missing: "缺失目标",
    constant: "常量",
    cycle: "循环",
  };
  const ROLE_TONE = {
    base: "base",
    business: "business",
    root: "root",
    bridge: "bridge",
    satellite: "satellite",
    orphan: "orphan",
    missing: "missing",
    constant: "constant",
    cycle: "danger",
  };

  let state = {
    project: null,
    graph: null,
    selected: null,
    query: "",
    role: "all",
    onlyIssues: false,
    labels: false,
    zoom: 1,
    autoFit: true,
    layout: null,
    nodeEls: new Map(),
    edgeEls: [],
  };

  async function api(cmd, args) {
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) throw new Error("未连接 Rust 侧");
    return invoke(cmd, args);
  }

  function esc(value) {
    return String(value ?? "").replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[c]);
  }

  function roleLabel(node) {
    if (node.role === "business" && node.layer) return `第 ${node.layer} 层`;
    return ROLE_LABEL[node.role] || node.role;
  }

  function roleTag(node) {
    if (node.role === "business") return `L${node.layer}`;
    if (node.role === "base") return "基表";
    return ROLE_LABEL[node.role] || node.role;
  }

  function isIssue(node) {
    return node.role === "orphan" || node.role === "missing" || hasBrokenOut(node);
  }

  function hasBrokenOut(node) {
    return state.graph?.edges.some((e) => e.from === node.id && e.broken);
  }

  function nodeById(id) {
    return state.graph?.nodes.find((n) => n.id === id) || null;
  }

  function filterNode(node) {
    if (state.onlyIssues && !isIssue(node)) return false;
    if (state.role !== "all") {
      if (state.role === "issues" && !isIssue(node)) return false;
      else if (state.role !== "issues" && node.role !== state.role) return false;
    }
    const q = state.query.trim().toLowerCase();
    if (!q) return true;
    return [node.id, node.group, roleLabel(node), ...(node.field_names || [])]
      .join(" ").toLowerCase().includes(q);
  }

  // ── 布局 ───────────────────────────────────────────────
  function buildLayout() {
    const nodes = state.graph.nodes;
    const layerNodes = nodes.filter((n) => !["bridge", "satellite", "orphan", "missing"].includes(n.role));
    const bridges = nodes.filter((n) => n.role === "bridge");
    const satellites = nodes.filter((n) => n.role === "satellite");
    const tray = nodes.filter((n) => n.role === "orphan" || n.role === "missing");
    const byLayer = new Map();
    for (const node of layerNodes) {
      const layer = node.layer || 1;
      if (!byLayer.has(layer)) byLayer.set(layer, []);
      byLayer.get(layer).push(node);
    }
    const maxLayer = Math.max(1, ...[...byLayer.keys()], ...bridges.map((n) => n.layer || 1));
    const baseY = 92 + (maxLayer - 1) * ROW_GAP;
    const positions = new Map();
    const levels = [];

    const satByOwner = new Map();
    for (const node of satellites) {
      if (!satByOwner.has(node.owner)) satByOwner.set(node.owner, []);
      satByOwner.get(node.owner).push(node);
    }
    const extraFor = (node) => (satByOwner.has(node.id) ? SAT_W + 16 : 0);
    const rowWidth = (items, w = W) => items.reduce((sum, node, i) => sum + w + extraFor(node) + (i ? GAP : 0), 0);
    let width = 900;
    for (const items of byLayer.values()) width = Math.max(width, rowWidth(items) + 120);
    width = Math.max(width, rowWidth(bridges) + 160, rowWidth(tray) + 120);

    const placeRow = (items, y, xLeft, w, h) => {
      const total = rowWidth(items, w);
      let cursor = xLeft + Math.max(0, (width - xLeft * 2 - total) / 2);
      items.forEach((node) => {
        positions.set(node.id, { x: cursor, y, w, h });
        const sats = satByOwner.get(node.id) || [];
        sats.forEach((sat, idx) => {
          positions.set(sat.id, {
            x: cursor + w + 16,
            y: y + 4 + idx * (SAT_H + 6),
            w: SAT_W,
            h: SAT_H,
          });
        });
        cursor += w + extraFor(node) + GAP;
      });
    };

    for (let layer = 1; layer <= maxLayer; layer++) {
      const items = (byLayer.get(layer) || []).slice().sort((a, b) => a.group.localeCompare(b.group) || a.id.localeCompare(b.id));
      const y = baseY - (layer - 1) * ROW_GAP;
      placeRow(items, y, 66, W, H);
      levels.push({ layer, y, count: items.length });
    }

    const bridgeY = maxLayer >= 2 ? baseY - ROW_GAP / 2 : baseY - ROW_GAP * 0.6;
    if (bridges.length) {
      const items = bridges.slice().sort((a, b) => a.id.localeCompare(b.id));
      placeRow(items, bridgeY, 66, W, H);
    }

    // 附属表已经在 placeRow 中跟所属表一起占位，避免压到下一张表。

    const trayY = baseY + ROW_GAP * 0.92;
    if (tray.length) {
      const items = tray.slice().sort((a, b) => (a.role === b.role ? a.id.localeCompare(b.id) : a.role.localeCompare(b.role)));
      placeRow(items, trayY, 66, W, H);
    }

    const height = Math.max(560, trayY + H + 80);
    return { positions, width, height, levels, bridgeY, trayY, hasTray: tray.length > 0, hasBridge: bridges.length > 0 };
  }

  // ── 渲染 ───────────────────────────────────────────────
  function renderSummary() {
    const s = state.graph?.summary;
    if (!s) return;
    $("dep-summary").innerHTML = `
      <span><b>${s.nodes}</b> 节点</span>
      <span><b>${s.edges}</b> 引用</span>
      <span class="warn"><b>${s.unconnected}</b> 未接入</span>
      <span class="danger"><b>${s.broken}</b> 断引用</span>
      <span><b>${s.layers}</b> 层</span>`;
  }

  function renderGraph() {
    const layout = state.layout;
    const canvas = $("dep-canvas");
    const space = $("dep-canvas-space");
    const svg = $("dep-edges");
    canvas.style.width = `${layout.width}px`;
    canvas.style.height = `${layout.height}px`;
    space.style.width = `${layout.width * state.zoom}px`;
    space.style.height = `${layout.height * state.zoom}px`;
    canvas.style.transform = `scale(${state.zoom})`;
    svg.setAttribute("viewBox", `0 0 ${layout.width} ${layout.height}`);
    svg.setAttribute("width", layout.width);
    svg.setAttribute("height", layout.height);

    let levels = layout.levels.map((l) => `
      <div class="dep-level-line" style="top:${l.y - 14}px"></div>
      <div class="dep-level-label" style="top:${l.y - 30}px">第 ${l.layer} 层 · ${l.count}</div>`).join("");
    if (layout.hasBridge) {
      levels += `<div class="dep-level-line bridge" style="top:${layout.bridgeY - 14}px"></div>
        <div class="dep-level-label bridge" style="top:${layout.bridgeY - 30}px">层间桥接</div>`;
    }
    if (layout.hasTray) {
      levels += `<div class="dep-tray" style="top:${layout.trayY - 32}px;height:${H + 58}px"></div>
        <div class="dep-tray-label" style="top:${layout.trayY - 26}px">未接入 / 缺失目标</div>`;
    }

    const nodes = state.graph.nodes.map((node) => {
      const pos = layout.positions.get(node.id);
      if (!pos) return "";
      const broken = hasBrokenOut(node);
      const role = node.role === "business" ? "business" : node.role;
      const tag = roleTag(node);
      const flag = broken && node.role !== "missing" ? '<span class="dep-node-flag">断引用</span>' : "";
      return `<button class="dep-node role-${esc(role)}${node.role === "satellite" ? " satellite" : ""}${broken ? " broken" : ""}"
        type="button" data-node="${esc(node.id)}" title="${esc(node.id)} · ${esc(roleLabel(node))}"
        style="left:${pos.x}px;top:${pos.y}px;width:${pos.w}px;height:${pos.h}px">
        <span class="dep-node-icon">${node.kind === "enum" ? "◇" : node.kind === "constant" ? "{}" : "▦"}</span>
        <span class="dep-node-main"><span class="dep-node-name">${esc(node.id)}</span>
          <span class="dep-node-sub">${esc(node.group || "—")} · ${node.rows} 行</span></span>
        <span class="dep-node-role">${esc(tag)}</span>${flag}
      </button>`;
    }).join("");

    const marker = `<defs>
      <marker id="dep-arrow" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="var(--border-strong)"/></marker>
      <marker id="dep-arrow-broken" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="var(--danger)"/></marker>
    </defs>`;

    const paths = state.graph.edges.map((edge) => {
      const from = layout.positions.get(edge.from);
      const to = layout.positions.get(edge.to);
      if (!from || !to) return "";
      let d;
      if (edge.kind === "attach") {
        d = `M ${from.x + from.w} ${from.y + from.h / 2} L ${to.x} ${to.y + to.h / 2}`;
      } else {
        const sx = from.x + from.w / 2;
        const sy = from.y + from.h;
        const tx = to.x + to.w / 2;
        const ty = to.y;
        const dir = ty >= sy ? 1 : -1;
        const bend = Math.max(42, Math.abs(ty - sy) * 0.42);
        d = `M ${sx} ${sy} C ${sx} ${sy + dir * bend}, ${tx} ${ty - dir * bend}, ${tx} ${ty}`;
      }
      const markerAttr = edge.kind === "broken"
        ? ' marker-end="url(#dep-arrow-broken)"'
        : edge.kind === "attach" || edge.kind === "bridge" ? "" : ' marker-end="url(#dep-arrow)"';
      const cls = `dep-edge ${esc(edge.kind)}${edge.broken ? " broken" : ""}`;
      const key = `${edge.from}|${edge.to}`;
      return `<path class="${cls}" d="${d}" data-edge="${esc(key)}"${markerAttr}></path>
        <text class="dep-edge-label ${esc(edge.kind)}" data-label="${esc(key)}">${esc(edge.label)}</text>`;
    }).join("");

    $("dep-nodes").innerHTML = `<div class="dep-layers">${levels}</div>${nodes}`;
    svg.innerHTML = `${marker}${paths}`;

    state.nodeEls = new Map([...$("dep-nodes").querySelectorAll("[data-node]")].map((el) => [el.dataset.node, el]));
    state.nodeEls.forEach((el, id) => el.addEventListener("click", () => select(id)));
    state.edgeEls = [];
    for (const edge of state.graph.edges) {
      const key = `${edge.from}|${edge.to}`;
      const path = svg.querySelector(`path[data-edge="${cssEscape(key)}"]`);
      const label = svg.querySelector(`text[data-label="${cssEscape(key)}"]`);
      if (!path || !label) continue;
      const point = path.getPointAtLength(path.getTotalLength() * 0.5);
      label.setAttribute("x", point.x);
      label.setAttribute("y", point.y - 6);
      path.addEventListener("mouseenter", () => label.classList.add("hovered"));
      path.addEventListener("mouseleave", () => label.classList.remove("hovered"));
      state.edgeEls.push({ edge, path, label });
    }
    applyFilters();
    updateSelection();
  }

  function cssEscape(value) {
    return window.CSS?.escape ? CSS.escape(value) : value.replace(/"/g, '\\"');
  }

  function renderSidebar() {
    const list = $("dep-node-list");
    const groups = new Map();
    for (const node of state.graph.nodes) {
      if (!filterNode(node)) continue;
      if (!groups.has(node.group || "—")) groups.set(node.group || "—", []);
      groups.get(node.group || "—").push(node);
    }
    let html = "";
    for (const [group, nodes] of [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
      html += `<div class="dep-group">${esc(group)}<span>${nodes.length}</span></div>`;
      for (const node of nodes.sort((a, b) => (a.layer || 0) - (b.layer || 0) || a.id.localeCompare(b.id))) {
        html += `<button class="dep-tree-node role-${esc(node.role)}${node.id === state.selected ? " active" : ""}" type="button" data-side="${esc(node.id)}">
          <span class="dep-tree-dot"></span><span class="dep-tree-name">${esc(node.id)}</span>
          <span class="dep-tree-role">${esc(roleTag(node))}</span>
        </button>`;
      }
    }
    list.innerHTML = html || '<div class="dep-empty">没有匹配的节点</div>';
    $("dep-node-count").textContent = String([...groups.values()].reduce((n, xs) => n + xs.length, 0));
    list.querySelectorAll("[data-side]").forEach((el) => el.addEventListener("click", () => select(el.dataset.side)));
  }

  function renderInspector() {
    const el = $("dep-inspector");
    const node = nodeById(state.selected);
    if (!node) {
      el.innerHTML = '<div class="dep-empty">选择一个节点查看依赖</div>';
      return;
    }
    const out = state.graph.edges.filter((e) => e.from === node.id);
    const incoming = state.graph.edges.filter((e) => e.to === node.id);
    const broken = hasBrokenOut(node);
    const tone = ROLE_TONE[node.role] || "";
    const hint = dependencyHint(node);
    const fields = (node.field_names || []).slice(0, 16);
    const depRow = (edge, outgoing) => {
      const other = outgoing ? edge.to : edge.from;
      const target = nodeById(other);
      return `<button class="dep-dep-row" type="button" data-go="${esc(other)}">
        <span class="dep-dep-name">${outgoing ? "→" : "←"} ${esc(other)}</span>
        <span class="dep-dep-kind">${esc(target ? roleLabel(target) : "目标")}</span>
        <span class="dep-dep-fields">${esc(edge.label)}</span>
      </button>`;
    };
    el.innerHTML = `
      <div class="dep-ins-kind role-${esc(node.role)}">${esc(roleLabel(node))}</div>
      <h2 class="dep-ins-title">${esc(node.id)}</h2>
      <div class="dep-ins-sub">${esc(node.group || "—")} · ${node.rows} 行 · ${node.fields} 字段</div>
      <div class="dep-ins-status ${broken ? "danger" : node.role === "orphan" || node.role === "missing" ? "warn" : ""}">
        <span></span>${esc(statusText(node, broken))}
      </div>
      <div class="dep-metrics">
        <div><span>层级</span><b>${node.layer ? node.layer : "—"}</b></div>
        <div><span>被引用</span><b>${node.incoming}</b></div>
        <div><span>引用</span><b>${node.outgoing}</b></div>
      </div>
      <div class="dep-ins-section"><div class="dep-ins-head">它引用 <span>${out.length}</span></div>
        <div class="dep-list">${out.map((e) => depRow(e, true)).join("") || '<div class="dep-empty">没有向外引用</div>'}</div></div>
      <div class="dep-ins-section"><div class="dep-ins-head">引用它的 <span>${incoming.length}</span></div>
        <div class="dep-list">${incoming.map((e) => depRow(e, false)).join("") || '<div class="dep-empty">没有被其它表引用</div>'}</div></div>
      <div class="dep-ins-section"><div class="dep-ins-head">字段 <span>${node.fields}</span></div>
        <div class="dep-field-list">${fields.map((f) => `<span>${esc(f)}</span>`).join("") || '<div class="dep-empty">—</div>'}</div></div>
      <div class="dep-ins-section"><div class="dep-ins-head">规则 <span>${(node.rules || []).length}</span></div>
        <div class="dep-rule-list">${(node.rules || []).map((r) => `<code>${esc(r)}</code>`).join("") || '<div class="dep-empty">没有自定义规则</div>'}</div></div>
      <div class="dep-hint ${tone}"><b>设计提示</b><p>${esc(hint)}</p></div>`;
    el.querySelectorAll("[data-go]").forEach((b) => b.addEventListener("click", () => select(b.dataset.go)));
  }

  function statusText(node, broken) {
    if (node.role === "missing") return "引用目标不存在";
    if (node.role === "orphan") return "无入边 + 无出边，疑似残留";
    if (node.role === "cycle") return "检测到循环依赖";
    if (broken) return "存在断引用";
    if (node.role === "satellite") return `附属表：仅 ${node.owner || "所属表"} 引用`;
    if (node.role === "bridge") return "桥接表：引用跨层目标";
    if (node.role === "root") return "无入边，未标记为入口";
    if (node.role === "constant") return "常量节点，不参与残留判定";
    return "依赖关系正常";
  }

  function dependencyHint(node) {
    switch (node.role) {
      case "orphan": return "没有任何入边或出边。先确认是否仍被脚本/运行时代码直接读取，否则可视为残留表。";
      case "missing": return "引用目标不存在；请检查字段类型里的 @目标 是否拼错，或目标表是否被删除/改名。";
      case "satellite": return "仅有一个消费者且没有出边，作为附属候选挂在所属表旁；建议确认归属后再决定是否合并或保留。";
      case "bridge": return "引用跨多个层级，属于桥接表；修改它会同时影响两端依赖，建议单独评审。";
      case "root": return "没有入边，但也没有显式入口标记；请确认它是否被运行时直接读取，避免误把正常入口当残留。";
      case "constant": return "常量不参与引用链，适合放全局静态值；如果字段只有一处使用，可以考虑降级为普通表列。";
      case "base": return "基表：没有出边，被上层共享引用。修改类型或删除前先看被引用方。";
      case "cycle": return "存在循环依赖；层级只能近似展示，建议优先拆分循环中的表。";
      default: return "按最长引用链深度归层；点击左侧节点查看上下游。";
    }
  }

  function updateSelection() {
    if (!state.graph) return;
    const selected = state.selected;
    const related = new Set([selected]);
    for (const edge of state.graph.edges) {
      if (edge.from === selected) related.add(edge.to);
      if (edge.to === selected) related.add(edge.from);
    }
    state.nodeEls.forEach((el, id) => {
      el.classList.toggle("selected", id === selected);
      el.classList.toggle("related", id !== selected && related.has(id));
      el.classList.toggle("dim", id !== selected && !related.has(id));
    });
    state.edgeEls.forEach(({ edge, path, label }) => {
      const hit = edge.from === selected || edge.to === selected;
      const near = related.has(edge.from) && related.has(edge.to);
      path.classList.toggle("selected", hit);
      path.classList.toggle("related", !hit && near);
      path.classList.toggle("dim", !hit && !near);
      label.classList.toggle("selected", hit);
      label.classList.toggle("related", !hit && near);
      label.classList.toggle("dim", !hit && !near);
    });
    renderInspector();
    renderSidebar();
  }

  function applyFilters() {
    if (!state.graph) return;
    for (const [id, el] of state.nodeEls) {
      const node = nodeById(id);
      el.hidden = !node || !filterNode(node);
    }
    for (const { edge, path, label } of state.edgeEls) {
      const from = nodeById(edge.from);
      const to = nodeById(edge.to);
      const hide = !from || !to || !filterNode(from) || !filterNode(to);
      path.hidden = hide;
      label.hidden = hide;
    }
    renderSidebar();
  }

  function select(id) {
    if (!nodeById(id)) return;
    state.selected = id;
    updateSelection();
  }

  function setZoom(value, fit = false) {
    state.zoom = Math.max(0.45, Math.min(1.5, value));
    state.autoFit = fit;
    if (state.layout) {
      $("dep-canvas").style.transform = `scale(${state.zoom})`;
      $("dep-canvas-space").style.width = `${state.layout.width * state.zoom}px`;
      $("dep-canvas-space").style.height = `${state.layout.height * state.zoom}px`;
    }
    $("dep-zoom").textContent = `${Math.round(state.zoom * 100)}%`;
  }

  function fitZoom() {
    if (!state.layout) return;
    const scroller = $("dep-scroll");
    const w = scroller.clientWidth - 28;
    const h = scroller.clientHeight - 28;
    if (w <= 0 || h <= 0) return;
    setZoom(Math.max(0.72, Math.min(w / state.layout.width, h / state.layout.height)), true);
  }

  function renderAll() {
    state.layout = buildLayout();
    renderSummary();
    renderGraph();
    renderSidebar();
    renderInspector();
    setZoom(state.zoom, state.autoFit);
  }

  async function refresh() {
    if (!state.project) return;
    const overlay = $("dependency-overlay");
    overlay.hidden = false;
    $("dep-status").textContent = "分析中…";
    try {
      const graph = await api("dependency_graph", { project: state.project });
      state.graph = graph;
      const preferred = graph.nodes.find((n) => n.role === "business")
        || graph.nodes.find((n) => n.role === "root")
        || graph.nodes[0];
      state.selected = preferred?.id || null;
      renderAll();
      $("dep-status").textContent = `项目 ${graph.project_name || graph.project_id} · ${graph.summary.nodes} 个节点`;
      requestAnimationFrame(fitZoom);
    } catch (e) {
      $("dep-status").textContent = String(e);
      $("dep-inspector").innerHTML = `<div class="dep-empty">${esc(e)}</div>`;
      $("dep-node-list").innerHTML = "";
      $("dep-nodes").innerHTML = "";
      $("dep-edges").innerHTML = "";
    }
  }

  function open(project) {
    state.project = project;
    refresh();
  }

  /** 左键拖空白平移；滚轮以鼠标位置为中心缩放。 */
  function initPanAndWheel() {
    const scroller = $("dep-scroll");
    let panning = null;
    scroller.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      if (e.target.closest(".dep-node")) return;
      panning = {
        id: e.pointerId,
        x: e.clientX,
        y: e.clientY,
        left: scroller.scrollLeft,
        top: scroller.scrollTop,
      };
      scroller.classList.add("panning");
      scroller.setPointerCapture?.(e.pointerId);
      e.preventDefault();
    });
    scroller.addEventListener("pointermove", (e) => {
      if (!panning || e.pointerId !== panning.id) return;
      scroller.scrollLeft = panning.left - (e.clientX - panning.x);
      scroller.scrollTop = panning.top - (e.clientY - panning.y);
    });
    const endPan = (e) => {
      if (!panning || (e && e.pointerId !== panning.id)) return;
      panning = null;
      scroller.classList.remove("panning");
    };
    scroller.addEventListener("pointerup", endPan);
    scroller.addEventListener("pointercancel", endPan);
    scroller.addEventListener("wheel", (e) => {
      if (!state.layout) return;
      e.preventDefault();
      const rect = scroller.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const before = state.zoom;
      const canvasX = (scroller.scrollLeft + mx) / before;
      const canvasY = (scroller.scrollTop + my) / before;
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      setZoom(before * factor, false);
      scroller.scrollLeft = canvasX * state.zoom - mx;
      scroller.scrollTop = canvasY * state.zoom - my;
    }, { passive: false });
  }

  function init() {
    $("dep-close").addEventListener("click", () => ($("dependency-overlay").hidden = true));
    $("dependency-overlay").addEventListener("click", (e) => {
      if (e.target === $("dependency-overlay")) $("dependency-overlay").hidden = true;
    });
    $("dep-refresh").addEventListener("click", refresh);
    $("dep-search").addEventListener("input", (e) => {
      state.query = e.target.value;
      applyFilters();
    });
    $("dep-role-filter").addEventListener("change", (e) => {
      state.role = e.target.value;
      applyFilters();
    });
    $("dep-only-issues").addEventListener("change", (e) => {
      state.onlyIssues = e.target.checked;
      applyFilters();
    });
    $("dep-labels").addEventListener("click", (e) => {
      state.labels = !state.labels;
      e.currentTarget.classList.toggle("active", state.labels);
      $("dep-canvas").classList.toggle("show-labels", state.labels);
    });
    initPanAndWheel();
    $("dep-zoom-in").addEventListener("click", () => setZoom(state.zoom + 0.1));
    $("dep-zoom-out").addEventListener("click", () => setZoom(state.zoom - 0.1));
    $("dep-zoom").addEventListener("click", () => setZoom(1));
    $("dep-fit").addEventListener("click", fitZoom);
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && !$("dependency-overlay").hidden) {
        $("dependency-overlay").hidden = true;
      }
    });
  }

  window.DependencyView = { open, refresh, init };
})();
