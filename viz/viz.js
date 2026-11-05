/* Renders a VizSpec (see Console/Sources/ConsoleCore/VizSpec.swift) into #app.
 * Entry point: window.renderViz(spec). Every string from a spec is written with textContent. */
(function () {
  "use strict";

  var css = getComputedStyle(document.documentElement);
  function token(name) { return css.getPropertyValue("--" + name).trim(); }

  var app = document.getElementById("app");
  var mermaidCounter = 0;
  var charts = [];
  var mermaidReady = false;

  function el(tag, className, text) {
    var node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined && text !== null) node.textContent = text;
    return node;
  }

  function clear() {
    charts.forEach(function (chart) { chart.destroy(); });
    charts = [];
    while (app.firstChild) app.removeChild(app.firstChild);
  }

  function stateView(title, detail, isError, source) {
    var box = el("section", "state enter" + (isError ? " error" : ""));
    box.setAttribute("role", isError ? "alert" : "status");
    box.appendChild(el("h2", null, title));
    if (detail) box.appendChild(el("p", isError ? "message" : "", detail));
    if (source) box.appendChild(el("pre", "code", source));
    app.appendChild(box);
  }

  function initMermaid() {
    if (mermaidReady || !window.mermaid) return;
    window.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      fontFamily: token("font") || "Geist",
      themeVariables: {
        darkMode: true,
        background: token("neutral"),
        primaryColor: token("surface-raised"),
        primaryTextColor: token("on-surface"),
        primaryBorderColor: token("border"),
        secondaryColor: token("surface"),
        secondaryTextColor: token("on-surface"),
        tertiaryColor: token("surface"),
        lineColor: token("muted"),
        textColor: token("on-surface"),
        mainBkg: token("surface-raised"),
        nodeBorder: token("border"),
        clusterBkg: token("surface"),
        clusterBorder: token("border"),
        edgeLabelBackground: token("neutral"),
        attributeBackgroundColorOdd: token("surface"),
        attributeBackgroundColorEven: token("surface-raised"),
        fontSize: "12px"
      }
    });
    mermaidReady = true;
  }

  function renderMermaid(source, host, onError, onDone) {
    initMermaid();
    if (!window.mermaid) { onError(new Error("Không tải được mermaid.")); return; }
    var id = "mermaid-" + (++mermaidCounter);
    window.mermaid.render(id, source).then(function (result) {
      host.innerHTML = result.svg;
      if (onDone) onDone(host.querySelector("svg"));
    }).catch(function (error) {
      // mermaid leaves a broken node behind on a parse error.
      var stray = document.getElementById("d" + id);
      if (stray && stray.parentNode) stray.parentNode.removeChild(stray);
      onError(error);
    });
  }

  /* "mcp__ccd_turn__update_plan" reads better as "update_plan". */
  function shortTool(name) { return String(name).replace(/^mcp__[^_]+(?:_[^_]+)*?__/, ""); }

  function formatTime(ms) {
    if (!ms) return "";
    var d = new Date(ms);
    var pad = function (n) { return (n < 10 ? "0" : "") + n; };
    return pad(d.getHours()) + ":" + pad(d.getMinutes());
  }

  /* ---------- flow ---------- */
  function renderFlow(flow) {
    var root = el("div", "enter");
    var head = el("div", "flow-head");
    if (flow.branch) {
      head.appendChild(el("span", "label muted", "Nhánh"));
      head.appendChild(el("span", "chip", flow.branch));
    }
    head.appendChild(el("span", "label muted", flow.totalCalls + " lần gọi công cụ, " + flow.turns.length + " lượt"));
    root.appendChild(head);

    if (flow.diffStat) {
      root.appendChild(el("h3", "panel-title", "git diff --stat"));
      root.appendChild(el("pre", "diffstat code", flow.diffStat));
    }
    if (flow.files.length) {
      root.appendChild(el("h3", "panel-title", "File đã sửa (" + flow.files.length + ")"));
      var chips = el("div", "chips");
      flow.files.forEach(function (file) {
        var chip = el("span", "chip", file.name + (file.count > 1 ? "  x" + file.count : ""));
        chip.title = file.path;
        chips.appendChild(chip);
      });
      root.appendChild(chips);
    }

    root.appendChild(el("h3", "panel-title", "Dòng thời gian"));
    flow.turns.forEach(function (turn) {
      var section = el("section", "turn");
      var turnHead = el("div", "turn-head");
      turnHead.appendChild(el("span", "turn-number", "#" + turn.index));
      turnHead.appendChild(el("span", "turn-prompt", turn.prompt || "(không có lời nhắc)"));
      section.appendChild(turnHead);
      var list = el("ul", "timeline");
      turn.calls.forEach(function (call) {
        var classes = "call" + (call.subagent ? " agent" : "") + (call.edit ? " edit" : "");
        var item = el("li", classes);
        item.appendChild(el("span", "tool", call.subagent ? "subagent" : shortTool(call.tool)));
        item.appendChild(el("span", "target", call.target));
        item.appendChild(el("span", "time", formatTime(call.time)));
        list.appendChild(item);
      });
      section.appendChild(list);
      root.appendChild(section);
    });
    app.appendChild(root);
  }

  /* ---------- zoom and pan ---------- */
  var MIN_DEFAULT_SCALE = 0.6;

  function button(label, title, onClick) {
    var node = el("button", "btn", label);
    node.type = "button";
    node.title = title;
    node.setAttribute("aria-label", title);
    node.addEventListener("click", onClick);
    return node;
  }

  /* A viewport around an SVG with pinch / ctrl+wheel zoom, wheel and drag pan, and a toolbar.
   * Default view: fit the width but never below 60 %; if that still overflows, start at the top-left. */
  function createViewer(svg, bar) {
    var viewport = el("div", "viewport");
    viewport.tabIndex = 0;
    viewport.setAttribute("role", "img");
    viewport.setAttribute("aria-label", "Sơ đồ. Kéo để di chuyển, ctrl + cuộn hoặc chụm hai ngón để thu phóng.");
    var stage = el("div", "stage");
    viewport.appendChild(stage);

    var box = svg.viewBox && svg.viewBox.baseVal && svg.viewBox.baseVal.width ? svg.viewBox.baseVal : null;
    var natural = box ? { w: box.width, h: box.height } : { w: svg.getBoundingClientRect().width, h: svg.getBoundingClientRect().height };
    svg.removeAttribute("style");
    svg.setAttribute("width", natural.w);
    svg.setAttribute("height", natural.h);
    stage.appendChild(svg);

    var view = { s: 1, x: 0, y: 0 };
    var label = el("span", "label muted zoom-label");
    var movedSinceDown = false;

    function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
    function apply() {
      stage.style.transform = "translate(" + view.x + "px," + view.y + "px) scale(" + view.s + ")";
      label.textContent = Math.round(view.s * 100) + " %";
    }
    function size() { return { w: viewport.clientWidth, h: viewport.clientHeight }; }
    function zoomAt(factor, cx, cy) {
      var next = clamp(view.s * factor, 0.1, 4);
      var ratio = next / view.s;
      view.x = cx - (cx - view.x) * ratio;
      view.y = cy - (cy - view.y) * ratio;
      view.s = next;
      apply();
    }
    function zoomCentered(factor) { var v = size(); zoomAt(factor, v.w / 2, v.h / 2); }
    function defaultView() {
      var v = size();
      view.s = clamp(v.w / natural.w, MIN_DEFAULT_SCALE, 1);
      view.x = natural.w * view.s <= v.w ? (v.w - natural.w * view.s) / 2 : 0;
      view.y = 0;
      apply();
    }
    function fitAll() {
      var v = size();
      view.s = clamp(Math.min(v.w / natural.w, v.h / natural.h), 0.05, 1.5);
      view.x = (v.w - natural.w * view.s) / 2;
      view.y = (v.h - natural.h * view.s) / 2;
      apply();
    }
    function actual() { view.s = 1; var v = size(); view.x = natural.w <= v.w ? (v.w - natural.w) / 2 : 0; view.y = 0; apply(); }

    bar.appendChild(button("Vừa khung", "Vừa khung: thu cả sơ đồ vào khung", fitAll));
    bar.appendChild(button("100 %", "Về cỡ thật 100 %", actual));
    bar.appendChild(button("+", "Phóng to", function () { zoomCentered(1.25); }));
    bar.appendChild(button("−", "Thu nhỏ", function () { zoomCentered(0.8); }));
    bar.appendChild(label);

    viewport.addEventListener("wheel", function (event) {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        var rect = viewport.getBoundingClientRect();
        zoomAt(Math.exp(-event.deltaY * 0.01), event.clientX - rect.left, event.clientY - rect.top);
      } else {
        view.x -= event.deltaX;
        view.y -= event.deltaY;
        apply();
      }
    }, { passive: false });

    // Safari reports a trackpad pinch as gesture events rather than ctrl+wheel.
    var gestureBase = 1;
    viewport.addEventListener("gesturestart", function (event) { event.preventDefault(); gestureBase = event.scale || 1; });
    viewport.addEventListener("gesturechange", function (event) {
      event.preventDefault();
      var rect = viewport.getBoundingClientRect();
      var factor = (event.scale || 1) / gestureBase;
      gestureBase = event.scale || 1;
      zoomAt(factor, (event.clientX || rect.width / 2 + rect.left) - rect.left, (event.clientY || rect.height / 2 + rect.top) - rect.top);
    });

    var drag = null;
    viewport.addEventListener("pointerdown", function (event) {
      if (event.button !== 0) return;
      drag = { x: event.clientX, y: event.clientY, vx: view.x, vy: view.y };
      movedSinceDown = false;
    });
    window.addEventListener("pointermove", function (event) {
      if (!drag) return;
      var dx = event.clientX - drag.x, dy = event.clientY - drag.y;
      if (!movedSinceDown && Math.abs(dx) + Math.abs(dy) < 4) return;
      if (!movedSinceDown) viewport.classList.add("dragging");
      movedSinceDown = true;
      view.x = drag.vx + dx;
      view.y = drag.vy + dy;
      apply();
    });
    window.addEventListener("pointerup", function () {
      drag = null;
      viewport.classList.remove("dragging");
    });

    // Layout needs the viewport in the document before it can be measured.
    requestAnimationFrame(defaultView);
    return {
      viewport: viewport, stage: stage,
      wasDrag: function () { return movedSinceDown; },
      centerOn: function (node) {
        var v = size();
        var vr = viewport.getBoundingClientRect(), nr = node.getBoundingClientRect();
        view.x += v.w / 2 - (nr.left - vr.left + nr.width / 2);
        view.y += v.h / 2 - (nr.top - vr.top + nr.height / 2);
        apply();
      }
    };
  }

  /* ---------- diagram ---------- */
  function renderDiagram(diagram) {
    var root = el("div", "enter");
    var toolbar = el("div", "toolbar");
    var select = el("select");
    select.setAttribute("aria-label", "Chọn sơ đồ");
    diagram.blocks.forEach(function (block, index) {
      var option = el("option", null, block.label);
      option.value = String(index);
      select.appendChild(option);
    });
    toolbar.appendChild(select);
    toolbar.appendChild(el("span", "label muted", diagram.blocks.length + " sơ đồ, mới nhất trước"));
    var spacer = el("span", "grow");
    toolbar.appendChild(spacer);
    var zoomBar = el("div", "zoombar");
    toolbar.appendChild(zoomBar);
    root.appendChild(toolbar);
    var host = el("div", "canvas");
    root.appendChild(host);
    app.appendChild(root);

    function show(index) {
      var block = diagram.blocks[index];
      host.textContent = "";
      zoomBar.textContent = "";
      var scratch = el("div");
      renderMermaid(block.source, scratch, function (error) {
        host.textContent = "";
        var box = el("div", "state error");
        box.setAttribute("role", "alert");
        box.style.minHeight = "0";
        box.appendChild(el("h2", null, "Sơ đồ mermaid không hợp lệ"));
        box.appendChild(el("p", "message", String(error && error.message ? error.message : error).split("\n")[0]));
        box.appendChild(el("pre", "code", block.source));
        host.appendChild(box);
      }, function (svg) {
        var viewer = createViewer(svg, zoomBar);
        host.appendChild(viewer.viewport);
      });
    }
    select.addEventListener("change", function () { show(Number(select.value)); });
    show(0);
  }

  /* ---------- entity ---------- */
  function entityIdOf(node) {
    var match = /^entity-(.+)-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.exec(node.id || "");
    return match ? match[1] : null;
  }

  function renderEntity(entity) {
    var root = el("div", "enter");
    var toolbar = el("div", "toolbar");
    toolbar.appendChild(el("span", "label muted",
      entity.tableCount + " bảng, " + entity.relationCount + " quan hệ" +
      (entity.inferredCount ? " (" + entity.inferredCount + " suy luận từ tên cột, nét đứt)" : "")));
    toolbar.appendChild(el("span", "grow"));
    var zoomBar = el("div", "zoombar");
    toolbar.appendChild(zoomBar);
    root.appendChild(toolbar);

    var tablesSection = el("section", "tables");
    tablesSection.appendChild(el("h3", "panel-title", "Bảng (" + entity.tables.length + ")"));
    var chips = el("div", "chips table-chips");
    tablesSection.appendChild(chips);
    root.appendChild(tablesSection);

    var host = el("div", "canvas");
    root.appendChild(host);

    var files = el("section", "files");
    files.appendChild(el("h3", "panel-title", "File đã đọc (" + entity.files.length + ")"));
    var list = el("ul");
    entity.files.forEach(function (file) {
      var item = el("li");
      item.appendChild(el("span", "code", file.path));
      item.appendChild(el("span", "count label", file.tables + " bảng"));
      list.appendChild(item);
    });
    files.appendChild(list);
    (entity.warnings || []).forEach(function (warning) { files.appendChild(el("p", "label warning", warning)); });
    root.appendChild(files);
    app.appendChild(root);

    var viewer = null, focused = null, nodes = {}, edgeLines = [], edgeLabels = [];
    var chipById = {};
    entity.tables.forEach(function (table) {
      var chip = el("button", "chip chip-button", table.name);
      chip.type = "button";
      chip.title = table.columns + " cột";
      chip.setAttribute("aria-pressed", "false");
      chip.addEventListener("click", function () { setFocus(focused === table.id ? null : table.id, true); });
      chips.appendChild(chip);
      chipById[table.id] = chip;
    });

    /* Focus a table: it and the tables it shares a relation with stay solid, the rest drop to 30 %. */
    function setFocus(id, recenter) {
      focused = id;
      var related = {};
      var linkActive = entity.links.map(function (link) {
        var active = id !== null && (link.from === id || link.to === id);
        if (active) { related[link.from] = true; related[link.to] = true; }
        return active;
      });
      if (id !== null) related[id] = true;
      Object.keys(nodes).forEach(function (key) {
        nodes[key].style.opacity = id === null || related[key] ? "1" : "0.3";
      });
      edgeLines.forEach(function (line, index) { line.style.opacity = id === null || linkActive[index] ? "1" : "0.3"; });
      edgeLabels.forEach(function (text, index) { text.style.opacity = id === null || linkActive[index] ? "1" : "0.3"; });
      Object.keys(chipById).forEach(function (key) {
        var on = key === id;
        chipById[key].classList.toggle("active", on);
        chipById[key].setAttribute("aria-pressed", on ? "true" : "false");
      });
      if (id !== null && recenter && viewer && nodes[id]) viewer.centerOn(nodes[id]);
    }

    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && focused !== null) setFocus(null, false);
    });

    renderMermaid(entity.mermaid, el("div"), function (error) {
      host.textContent = "";
      var box = el("div", "state error");
      box.style.minHeight = "0";
      box.appendChild(el("h2", null, "Không vẽ được sơ đồ bảng"));
      box.appendChild(el("p", "message", String(error && error.message ? error.message : error).split("\n")[0]));
      box.appendChild(el("pre", "code", entity.mermaid));
      host.appendChild(box);
    }, function (svg) {
      viewer = createViewer(svg, zoomBar);
      host.appendChild(viewer.viewport);
      svg.querySelectorAll("g[id^='entity-']").forEach(function (node) {
        var id = entityIdOf(node);
        if (!id) return;
        nodes[id] = node;
        node.style.cursor = "pointer";
        node.style.transition = "opacity 150ms";
        node.addEventListener("click", function (event) {
          if (viewer.wasDrag()) return;
          event.stopPropagation();
          setFocus(focused === id ? null : id, false);
        });
      });
      edgeLines = Array.prototype.slice.call(svg.querySelectorAll("path.relationshipLine"));
      edgeLabels = entity.links.map(function (_, index) { return svg.querySelector("#rel" + (index + 1)); }).filter(Boolean);
      edgeLines.concat(edgeLabels).forEach(function (node) { node.style.transition = "opacity 150ms"; });
      viewer.viewport.addEventListener("click", function () {
        if (!viewer.wasDrag() && focused !== null) setFocus(null, false);
      });
    });
  }

  /* ---------- chart ---------- */
  function renderChart(chart) {
    if (!window.Chart) { stateView("Không tải được thư viện biểu đồ", null, true); return; }
    var root = el("div", "charts enter");
    var muted = token("muted"), onSurface = token("on-surface"), border = token("border");
    Chart.defaults.font.family = token("font") || "Geist";
    Chart.defaults.font.size = 11;
    Chart.defaults.color = muted;
    Chart.defaults.animation = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? false : { duration: 200 };

    function card(title, totals) {
      var box = el("section", "chart-card");
      box.appendChild(el("h3", "panel-title", title));
      if (totals && totals.length) {
        var line = el("div", "totals");
        totals.forEach(function (entry) {
          var span = el("span", null, entry[0] + " ");
          span.appendChild(el("b", null, entry[1]));
          line.appendChild(span);
        });
        box.appendChild(line);
      }
      var canvas = document.createElement("canvas");
      box.appendChild(canvas);
      root.appendChild(box);
      return canvas;
    }
    function fmt(n) { return Number(n).toLocaleString("en-US"); }
    // Tints of two tokens, so the series stay inside the palette and differ by lightness.
    function tint(color, alpha) {
      var hex = color.replace("#", "");
      var r = parseInt(hex.substr(0, 2), 16), g = parseInt(hex.substr(2, 2), 16), b = parseInt(hex.substr(4, 2), 16);
      return "rgba(" + r + "," + g + "," + b + "," + alpha + ")";
    }
    var primary = token("primary");
    var onSurface70 = tint(onSurface, 0.7);
    /* Canvas fill patterns, drawn from the same token colours. */
    function pattern(kind, color) {
      var tile = document.createElement("canvas");
      tile.width = tile.height = 8;
      var g = tile.getContext("2d");
      g.fillStyle = tint(color, kind === "solid" ? 0.4 : 0.18);
      g.fillRect(0, 0, 8, 8);
      if (kind === "hatch") {
        g.strokeStyle = tint(color, 0.9); g.lineWidth = 1;
        g.beginPath(); g.moveTo(0, 8); g.lineTo(8, 0); g.moveTo(-2, 2); g.lineTo(2, -2); g.moveTo(6, 10); g.lineTo(10, 6); g.stroke();
      } else if (kind === "dots") {
        g.fillStyle = tint(color, 0.9);
        g.beginPath(); g.arc(4, 4, 1.1, 0, Math.PI * 2); g.fill();
      }
      return g.createPattern(tile, "repeat");
    }
    var scaleStyle = { grid: { color: border }, border: { color: border }, ticks: { color: muted } };

    var timeTicks = { color: muted, maxRotation: 0, autoSkip: true, maxTicksLimit: 8 };
    if (chart.tokens.length) {
      var t = chart.totals;
      var labels = chart.tokens.map(function (p) { return formatTime(p.time); });
      function series(label, key, lineColor, fill, fillTo, dash, width) {
        return { label: label, data: chart.tokens.map(function (p) { return p[key]; }), fill: fillTo,
                 borderColor: lineColor, backgroundColor: fill, borderDash: dash, pointRadius: 0, borderWidth: width, tension: 0.2 };
      }
      // Cache reads are two orders of magnitude above the rest, so they get their own plot.
      var canvas = card("Token mới theo thời gian", [
        ["Vào", fmt(t.input)], ["Ra", fmt(t.output)], ["Cache ghi", fmt(t.cacheCreation)]
      ]);
      charts.push(new Chart(canvas, {
        type: "line",
        data: { labels: labels, datasets: [
          // Distinct by colour, by fill pattern and by line dash, so they read without colour:
          // output is the one accent (primary, solid), input is dotted, cache writes are hatched and dashed.
          series("Cache ghi", "cacheCreation", muted, pattern("hatch", muted), "origin", [5, 3], 1),
          series("Vào", "input", onSurface70, pattern("dots", onSurface), "-1", [1, 3], 1.25),
          series("Ra", "output", primary, pattern("solid", primary), "-1", [], 1.75)
        ] },
        options: { responsive: true, interaction: { mode: "index", intersect: false },
          scales: { x: Object.assign({}, scaleStyle, { ticks: timeTicks }), y: Object.assign({ stacked: true, beginAtZero: true }, scaleStyle) },
          plugins: { legend: { labels: { boxWidth: 14, boxHeight: 10, color: onSurface } } } }
      }));
      var cacheCanvas = card("Cache đọc theo thời gian", [["Tổng", fmt(t.cacheRead)]]);
      charts.push(new Chart(cacheCanvas, {
        type: "line",
        data: { labels: labels, datasets: [series("Cache đọc", "cacheRead", muted, tint(muted, 0.25), "origin", [], 1)] },
        options: { responsive: true, interaction: { mode: "index", intersect: false },
          scales: { x: Object.assign({}, scaleStyle, { ticks: timeTicks }), y: Object.assign({ beginAtZero: true }, scaleStyle) },
          plugins: { legend: { display: false } } }
      }));
    }

    function bar(title, rows, keyName) {
      if (!rows.length) return;
      var canvas = card(title);
      charts.push(new Chart(canvas, {
        type: "bar",
        data: { labels: rows.map(function (r) { return keyName === "name" ? shortTool(r[keyName]) : r[keyName]; }),
                datasets: [{ data: rows.map(function (r) { return r.count; }), backgroundColor: tint(muted, 0.7), borderColor: tint(muted, 1), borderWidth: 1, borderRadius: 4 }] },
        options: { indexAxis: "y", responsive: true,
          scales: { x: Object.assign({ beginAtZero: true, ticks: { precision: 0, color: muted } }, { grid: scaleStyle.grid, border: scaleStyle.border }),
                    y: { grid: { display: false }, border: scaleStyle.border, ticks: { color: onSurface, font: { family: token("font-mono") || "monospace" } } } },
          plugins: { legend: { display: false } } }
      }));
    }
    bar("Lần gọi theo công cụ", chart.tools, "name");
    bar("Subagent theo model", chart.models, "model");
    app.appendChild(root);
  }

  window.renderViz = function (spec) {
    clear();
    try {
      if (!spec || typeof spec !== "object") { stateView("Không có dữ liệu", null, true); return; }
      if (spec.error) { stateView(spec.error.title || "Có lỗi", spec.error.message, true, spec.error.source); return; }
      if (spec.empty) { stateView(spec.empty.title, spec.empty.detail, false); return; }
      switch (spec.kind) {
        case "flow": renderFlow(spec.flow); break;
        case "diagram": renderDiagram(spec.diagram); break;
        case "entity": renderEntity(spec.entity); break;
        case "chart": renderChart(spec.chart); break;
        default: stateView("Loại visualize không biết: " + spec.kind, null, true);
      }
    } catch (error) {
      clear();
      stateView("Không vẽ được", String(error && error.message ? error.message : error), true);
    }
  };

  document.documentElement.dataset.ready = "1";
})();
