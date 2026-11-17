const NB = (() => {
  const NS = "http://www.w3.org/2000/svg";
  const C = { orange: "#ff9a5a", green: "#7fd68b", blue: "#6fb5ff", ink: "#0a0f22", white: "#ffffff" };
  const TINT = { orange: "#ffc29a", green: "#b2e8b9", blue: "#a9d3ff" };
  const BG = {
    blue: { stops: [[0, TINT.blue], [1, C.blue]], dir: [0, 0, 0, 1] },
    orange: { stops: [[0, TINT.orange], [1, C.orange]], dir: [0, 0, 0, 1] },
    green: { stops: [[0, TINT.green], [1, C.green]], dir: [0, 0, 0, 1] },
    tri: { stops: [[0, C.orange], [0.5, C.green], [1, C.blue]], dir: [0, 1, 1, 0] }
  };
  const DOTS = [C.orange, C.green, C.blue];
  const NOTCH = "M100 100H924V150H660C636 150 628 158 624 182L618 214C612 244 594 258 562 258H462C430 258 412 244 406 214L400 182C396 158 388 150 364 150H100Z";
  const N = { L: { cx: 310, cy: 580, w: 120, h: 520, r: 0 }, D: { cx: 512, cy: 590, w: 120, h: 580, r: -37 }, R: { cx: 714, cy: 580, w: 120, h: 520, r: 0 } };
  const DOTS_N = [[310, 780], [512, 580], [714, 380]];
  let uid = 0;

  function grad(id, bg) {
    const g = BG[bg];
    return `<linearGradient id="${id}" x1="${g.dir[0]}" y1="${g.dir[1]}" x2="${g.dir[2]}" y2="${g.dir[3]}">${g.stops.map(s => `<stop offset="${s[0]}" stop-color="${s[1]}"/>`).join("")}</linearGradient>`;
  }

  function build(svg, bg, opts = {}) {
    const u = uid++;
    const square = opts.square;
    const clip = square ? `<rect x="0" y="0" width="1024" height="1024"/>` : `<rect x="100" y="100" width="824" height="824" rx="185"/>`;
    const area = square ? `x="0" y="0" width="1024" height="1024"` : `x="100" y="100" width="824" height="824"`;
    svg.setAttribute("viewBox", "0 0 1024 1024");
    svg.innerHTML = `<defs><clipPath id="nbc${u}">${clip}</clipPath>${grad("nbg" + u, bg)}
      <filter id="nbd${u}" x="-30%" y="-30%" width="160%" height="170%"><feDropShadow dx="0" dy="14" stdDeviation="12" flood-color="${C.ink}" flood-opacity=".28"/></filter>
      <filter id="nbw${u}" x="-80%" y="-80%" width="260%" height="260%"><feGaussianBlur stdDeviation="8" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>
      <g clip-path="url(#nbc${u})"><rect ${area} fill="url(#nbg${u})"/>
      ${opts.noNotch ? "" : `<path d="${NOTCH}" fill="${C.ink}" ${square ? 'transform="translate(-100 -100) scale(1.1953) translate(0 0)"' : ""}/>`}
      <g class="extra"></g>
      <g class="all"><g filter="url(#nbd${u})"><rect class="L"/><rect class="R"/><rect class="D"/></g>
      <g filter="url(#nbw${u})"><circle class="d0"/><circle class="d1"/><circle class="d2"/></g></g></g>`;
    return svg;
  }

  function draw(svg, P, dots, o = {}) {
    for (const k of ["L", "D", "R"]) {
      const p = P[k], e = svg.querySelector("." + k);
      e.setAttribute("x", p.cx - p.w / 2); e.setAttribute("y", p.cy - p.h / 2);
      e.setAttribute("width", p.w); e.setAttribute("height", p.h);
      e.setAttribute("rx", Math.min(p.w, p.h) * (p.k ?? 0.5));
      e.setAttribute("transform", `rotate(${p.r} ${p.cx} ${p.cy})`);
      e.setAttribute("fill", C.white); e.setAttribute("stroke", C.ink); e.setAttribute("stroke-width", "12");
    }
    dots.forEach((d, i) => {
      const e = svg.querySelector(".d" + i);
      e.setAttribute("cx", d[0]); e.setAttribute("cy", d[1]); e.setAttribute("r", d[2] ?? 26);
      e.setAttribute("opacity", d[3] ?? 1); e.setAttribute("fill", d[4] ?? DOTS[i]);
    });
    svg.querySelector(".all").setAttribute("transform", o.tf || "");
    svg.querySelector(".extra").innerHTML = o.extra || "";
  }

  const ease = t => t < .5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
  const lerp = (a, b, t) => a + (b - a) * t;
  const cl = (x, a, b) => Math.max(a, Math.min(b, x));
  const seg = (t, a, b) => ease(cl((t - a) / (b - a), 0, 1));
  const copy = o => JSON.parse(JSON.stringify(o));

  function mix(P0, P1, ta, tb) {
    const o = {};
    for (const k of ["L", "D", "R"]) {
      const a = P0[k], b = P1[k], t = k === "D" ? ta : tb;
      o[k] = { cx: lerp(a.cx, b.cx, t), cy: lerp(a.cy, b.cy, t), w: lerp(a.w, b.w, t), h: lerp(a.h, b.h, t), r: lerp(a.r, b.r, t), k: lerp(a.k ?? .5, b.k ?? .5, t) };
    }
    return o;
  }

  function morph(svg, shape, t) {
    const P = mix(N, shape.B, seg(t, 0, .5), seg(t, .25, .8));
    const c = seg(t, .5, 1);
    draw(svg, P, DOTS_N.map((d, i) => [lerp(d[0], shape.dots[i][0], c), lerp(d[1], shape.dots[i][1], c), lerp(26, 24, c)]));
    return P;
  }

  function logo(svg) { draw(svg, N, DOTS_N.map(d => [d[0], d[1], 30])); }

  function glyph(mono) {
    const f = mono ? "currentColor" : C.ink;
    const dot = i => mono ? "currentColor" : DOTS[i];
    return `<svg xmlns="${NS}" viewBox="0 0 22 16" width="22" height="16"><g fill="none" stroke="${f}" stroke-width="1.6"><rect x="3.2" y="2.2" width="3.6" height="11.6" rx="1.8"/><rect x="15.2" y="2.2" width="3.6" height="11.6" rx="1.8"/><rect x="9.2" y="1.4" width="3.6" height="13.2" rx="1.8" transform="rotate(-37 11 8)"/></g><circle cx="5" cy="12" r="1.3" fill="${dot(0)}"/><circle cx="11" cy="8" r="1.3" fill="${dot(1)}"/><circle cx="17" cy="4" r="1.3" fill="${dot(2)}"/></svg>`;
  }

  const SHAPES = [
    { id: "visor", t: "Visor", B: { L: { cx: 300, cy: 620, w: 96, h: 280, r: 30 }, D: { cx: 512, cy: 600, w: 300, h: 400, r: 0, k: .32 }, R: { cx: 724, cy: 620, w: 96, h: 280, r: -30 } }, dots: [[452, 500], [512, 500], [572, 500]], anim: "scan" },
    { id: "tower", t: "Signal tower", B: { L: { cx: 430, cy: 740, w: 110, h: 180, r: -70 }, D: { cx: 512, cy: 540, w: 170, h: 520, r: 0 }, R: { cx: 594, cy: 740, w: 110, h: 180, r: 70 } }, dots: [[512, 640], [512, 520], [512, 400]], anim: "cycle" },
    { id: "tripod", t: "Tripod", B: { L: { cx: 400, cy: 600, w: 110, h: 470, r: 30 }, D: { cx: 512, cy: 770, w: 110, h: 420, r: 90 }, R: { cx: 624, cy: 600, w: 110, h: 470, r: -30 } }, dots: [[360, 770], [512, 410], [664, 770]], anim: "pass" },
    { id: "drone", t: "Notch drone", B: { L: { cx: 300, cy: 470, w: 90, h: 260, r: -60 }, D: { cx: 512, cy: 560, w: 440, h: 160, r: 0 }, R: { cx: 724, cy: 470, w: 90, h: 260, r: 60 } }, dots: [[432, 560], [512, 560], [592, 560]], anim: "bob" },
    { id: "crew", t: "Three bots", B: { L: { cx: 330, cy: 660, w: 140, h: 260, r: 0 }, D: { cx: 512, cy: 620, w: 140, h: 300, r: 0 }, R: { cx: 694, cy: 660, w: 140, h: 260, r: 0 } }, dots: [[330, 590], [512, 540], [694, 590]], anim: "bounce" },
    { id: "walker", t: "Walker", B: { L: { cx: 450, cy: 770, w: 90, h: 240, r: 12 }, D: { cx: 512, cy: 520, w: 260, h: 360, r: 0, k: .36 }, R: { cx: 574, cy: 770, w: 90, h: 240, r: -12 } }, dots: [[452, 520], [512, 520], [572, 520]], anim: "walk" },
    { id: "bridge", t: "Bridge", B: { L: { cx: 330, cy: 690, w: 110, h: 300, r: 0 }, D: { cx: 512, cy: 500, w: 540, h: 110, r: 0 }, R: { cx: 694, cy: 690, w: 110, h: 300, r: 0 } }, dots: [[380, 500], [512, 500], [644, 500]], anim: "travel" },
    { id: "vault", t: "Padlock", B: { L: { cx: 410, cy: 470, w: 90, h: 280, r: 0 }, D: { cx: 512, cy: 670, w: 400, h: 300, r: 0, k: .25 }, R: { cx: 614, cy: 470, w: 90, h: 280, r: 0 } }, dots: [[442, 670], [512, 670], [582, 670]], anim: "lock" },
    { id: "rocket", t: "Rocket", B: { L: { cx: 405, cy: 700, w: 90, h: 230, r: 30 }, D: { cx: 512, cy: 550, w: 190, h: 480, r: 0 }, R: { cx: 619, cy: 700, w: 90, h: 230, r: -30 } }, dots: [[512, 650], [512, 550], [512, 450]], anim: "thrust" },
    { id: "hourglass", t: "Hourglass", B: { L: { cx: 512, cy: 560, w: 100, h: 520, r: 32 }, D: { cx: 512, cy: 830, w: 420, h: 80, r: 0 }, R: { cx: 512, cy: 560, w: 100, h: 520, r: -32 } }, dots: [[512, 740], [512, 560], [512, 380]], anim: "fall" },
    { id: "rack", t: "Machine rack", B: { L: { cx: 512, cy: 420, w: 110, h: 480, r: 90 }, D: { cx: 512, cy: 580, w: 110, h: 480, r: 90 }, R: { cx: 512, cy: 740, w: 110, h: 480, r: 90 } }, dots: [[680, 740], [680, 580], [680, 420]], anim: "leds" },
    { id: "swap", t: "Swap wheel", B: { L: { cx: 450, cy: 420, w: 100, h: 380, r: 90 }, D: { cx: 512, cy: 600, w: 240, h: 240, r: 0 }, R: { cx: 574, cy: 780, w: 100, h: 380, r: 90 } }, dots: [[452, 635], [512, 530], [572, 635]], anim: "swap" },
    { id: "tray", t: "Inbox tray", B: { L: { cx: 320, cy: 640, w: 100, h: 320, r: -22 }, D: { cx: 512, cy: 790, w: 480, h: 110, r: 0 }, R: { cx: 704, cy: 640, w: 100, h: 320, r: 22 } }, dots: [[442, 720], [512, 720], [582, 720]], anim: "drop" },
  ];

  function live(svg, shape, s) {
    const P = copy(mix(N, shape.B, 1, 1));
    let dy = 0, dx = 0;
    const off = { L: 0, D: 0, R: 0 };
    let dots = shape.dots.map(d => [d[0], d[1], 24, 1]);
    if (shape.anim === "scan") { const p = (Math.sin(s * 2.2) + 1); dots = dots.map((d, i) => { const o = Math.max(0, 1 - Math.abs(p - i)); return [d[0], d[1], 22 + 6 * o, .35 + .65 * o]; }); }
    if (shape.anim === "cycle") { const a = Math.floor(s * 1.2) % 3; dots = dots.map((d, i) => [d[0], d[1], i === a ? 30 : 22, i === a ? 1 : .3]); }
    if (shape.anim === "pass") { const p = (s * 1.4) % 3; dots = dots.map((d, i) => { const o = Math.max(0, 1 - Math.abs(p - i)); return [d[0], d[1], 22 + 8 * o, .35 + .65 * o]; }); }
    if (shape.anim === "bob") { dy = Math.sin(s * 2.4) * 14; dots = dots.map((d, i) => [d[0], d[1], 24, .6 + .4 * Math.max(0, Math.sin(s * 6 - i * 1.2))]); }
    if (shape.anim === "bounce") { ["L", "D", "R"].forEach((k, i) => { off[k] = -Math.max(0, Math.sin(s * 5 - i * 2.1)) * 26; P[k].cy += off[k]; }); dots = dots.map((d, i) => [d[0], d[1] + off[["L", "D", "R"][i]], 24, 1]); }
    if (shape.anim === "walk") { P.L.r += Math.sin(s * 5) * 14; P.R.r -= Math.sin(s * 5) * 14; dy = -Math.abs(Math.sin(s * 5)) * 10; const f = (s * .8) % 1; dots = dots.map((d, i) => [d[0], d[1], 24, f > i / 3 ? 1 : .3]); }
    if (shape.anim === "travel") { dots = dots.map((d, i) => { const f = ((s * .5) + i / 3) % 1; return [lerp(300, 724, f), d[1] - Math.sin(f * Math.PI) * 20, 24, f < .1 ? f * 10 : f > .9 ? (1 - f) * 10 : 1]; }); }
    if (shape.anim === "lock") { const o = Math.max(0, Math.sin(s * 1.6)) * 60; P.L.cy -= o; P.R.cy -= o; dots = dots.map((d, i) => [d[0], d[1], 24, o > 30 ? 1 : .35 + .65 * Math.max(0, Math.sin(s * 5 - i))]); }
    if (shape.anim === "thrust") { dy = Math.sin(s * 3) * 16; P.L.r += Math.sin(s * 9) * 3; P.R.r -= Math.sin(s * 9) * 3; dots = dots.map((d, i) => [d[0], d[1], 22, .4 + .6 * Math.max(0, Math.sin(s * 6 - i * 1.4))]); }
    if (shape.anim === "fall") { dots = dots.map((d, i) => { const f = ((s * .45) + i / 3) % 1; return [512, lerp(370, 770, f * f), 22, f > .92 ? (1 - f) * 12 : 1]; }); }
    if (shape.anim === "leds") { dots = dots.map((d, i) => { const on = Math.sin(s * (3 + i * 1.7) + i * 2) > -.2; return [d[0], d[1], on ? 26 : 20, on ? 1 : .25]; }); }
    if (shape.anim === "swap") { dots = dots.map((d, i) => { const a = s * 1.6 + i * 2.094 - Math.PI / 2; return [512 + Math.cos(a) * 66, 600 + Math.sin(a) * 66, 24, 1]; }); }
    if (shape.anim === "drop") { dots = dots.map((d, i) => { const f = ((s * .5) + i / 3) % 1; const y = lerp(340, d[1], ease(cl(f * 1.6, 0, 1))); return [d[0], y, 24, f > .85 ? (1 - f) * 6.6 : 1]; }); }
    draw(svg, P, dots, { tf: `translate(${dx} ${dy})` });
  }

  return { NS, C, BG, DOTS, N, DOTS_N, SHAPES, build, draw, mix, morph, live, logo, glyph, ease, lerp, cl, seg, copy };
})();
