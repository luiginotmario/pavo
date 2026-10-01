(() => {
  const NS = "http://www.w3.org/2000/svg";
  const INK = "#f1efe8";
  const BG = "#0c0c0d";
  const REDUCED = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // --- drawing helpers -------------------------------------------------------

  function pen(svg, seed) {
    const rc = rough.svg(svg);
    const base = { stroke: INK, strokeWidth: 1.5, roughness: 1.3, bowing: 1.1, seed };
    const add = (node) => (svg.appendChild(node), node);
    const o = (extra) => ({ ...base, ...extra });

    return {
      line: (x1, y1, x2, y2, e) => add(rc.line(x1, y1, x2, y2, o(e))),
      rect: (x, y, w, h, e) => add(rc.rectangle(x, y, w, h, o(e))),
      ellipse: (cx, cy, w, h, e) => add(rc.ellipse(cx, cy, w, h, o(e))),
      circle: (cx, cy, d, e) => add(rc.circle(cx, cy, d, o(e))),
      path: (d, e) => add(rc.path(d, o(e))),
      curve: (pts, e) => add(rc.curve(pts, o(e))),
      poly: (pts, e) => add(rc.linearPath(pts, o(e))),
      text: (x, y, str, { size = 22, anchor = "middle", font = "hand", fill = INK } = {}) => {
        const t = document.createElementNS(NS, "text");
        t.setAttribute("x", x);
        t.setAttribute("y", y);
        t.setAttribute("fill", fill);
        t.setAttribute("text-anchor", anchor);
        t.setAttribute("font-size", size);
        t.setAttribute(
          "font-family",
          font === "mono" ? "ui-monospace, SF Mono, Menlo, monospace" : "Caveat, Bradley Hand, cursive"
        );
        if (font !== "mono") t.setAttribute("font-weight", "700");
        t.textContent = str;
        return add(t);
      },
      group: (transform, fn) => {
        const g = document.createElementNS(NS, "g");
        g.setAttribute("transform", transform);
        svg.appendChild(g);
        fn(pen(g, seed + 1));
        return g;
      },
    };
  }

  function arrowHead(p, x, y, angle, size = 10) {
    const a1 = angle + Math.PI * 0.8;
    const a2 = angle - Math.PI * 0.8;
    p.poly([
      [x + Math.cos(a1) * size, y + Math.sin(a1) * size],
      [x, y],
      [x + Math.cos(a2) * size, y + Math.sin(a2) * size],
    ]);
  }

  function fileIcon(p, x, y, w, h, label) {
    const f = 12;
    p.path(`M${x} ${y} L${x + w - f} ${y} L${x + w} ${y + f} L${x + w} ${y + h} L${x} ${y + h} Z`, {
      fill: BG,
      fillStyle: "solid",
    });
    p.poly([[x + w - f, y], [x + w - f, y + f], [x + w, y + f]]);
    if (label) p.text(x + w / 2, y + h / 2 + 6, label, { size: 17 });
  }

  // tiny fanned tail — the menu bar icon
  function fan(p, cx, cy, r, dot = 3.5) {
    for (let i = 0; i < 5; i++) {
      const a = (-150 + i * 30) * (Math.PI / 180);
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      p.line(cx, cy, x, y, { strokeWidth: 1.2, roughness: 0.6 });
      p.circle(x, y, dot, { fill: INK, fillStyle: "solid", roughness: 0.4 });
    }
  }

  // --- hero peacock ----------------------------------------------------------

  function drawPeacock(svg) {
    const p = pen(svg, 7);
    const bx = 160;
    const by = 172;

    const feather = (deg, len, ew, eh) => {
      const a = (deg * Math.PI) / 180;
      const tx = bx + Math.cos(a) * len;
      const ty = by + Math.sin(a) * len;
      p.line(bx, by, tx, ty, { strokeWidth: 1.2 });
      p.group(`rotate(${deg + 90} ${tx} ${ty})`, (g) => {
        g.ellipse(tx, ty, ew, eh, { strokeWidth: 1.3 });
        g.ellipse(tx, ty + 2, ew * 0.42, eh * 0.42, { fill: INK, fillStyle: "solid", roughness: 0.6 });
      });
    };

    for (let i = 0; i < 9; i++) feather(-166 + i * 19, 126, 22, 30);
    for (let i = 0; i < 8; i++) feather(-156.5 + i * 19, 80, 14, 19);

    // body, neck and head are filled with the background so they sit in front of the tail
    p.ellipse(bx, 184, 44, 56, { fill: BG, fillStyle: "solid" });
    p.path("M150 168 C 148 142, 158 122, 166 104 L 177 107 C 172 126, 172 146, 171 168", {
      fill: BG,
      fillStyle: "solid",
    });
    p.circle(173, 99, 19, { fill: BG, fillStyle: "solid" });
    p.circle(176, 97, 3, { fill: INK, fillStyle: "solid", roughness: 0.3 });
    p.poly([[181, 96], [192, 100], [181, 104]]);
    for (const [x, y] of [[163, 76], [170, 73], [177, 76]]) {
      p.line(171, 90, x, y, { strokeWidth: 1.1, roughness: 0.6 });
      p.circle(x, y, 4, { fill: INK, fillStyle: "solid", roughness: 0.3 });
    }
    p.line(153, 209, 151, 228);
    p.line(167, 209, 169, 228);
    p.poly([[144, 230], [151, 228], [156, 231]]);
    p.poly([[164, 231], [169, 228], [176, 230]]);
  }

  // --- install steps -----------------------------------------------------------

  function drawDownload(p, label, ext) {
    p.line(130, 10, 130, 62);
    arrowHead(p, 130, 64, Math.PI / 2, 13);
    p.poly([[72, 118], [72, 152], [188, 152], [188, 118]]);
    fileIcon(p, 102, 72, 56, 68, ext);
    p.text(130, 182, label, { size: 24 });
  }

  function drawDragToApps(p) {
    p.rect(12, 12, 236, 150);
    p.line(12, 34, 248, 34);
    for (const x of [24, 36, 48]) p.circle(x, 23, 7);
    // app icon
    p.rect(40, 62, 58, 58, { fill: BG, fillStyle: "solid" });
    fan(p, 69, 110, 23);
    p.text(69, 146, "cambio", { size: 21 });
    // applications folder
    p.path("M162 76 L162 120 L222 120 L222 84 L196 84 L189 76 Z");
    p.line(162, 90, 222, 90, { roughness: 0.8 });
    p.text(192, 146, "applications", { size: 21 });
    // arrow
    p.curve([[104, 84], [131, 60], [156, 82]]);
    arrowHead(p, 157, 84, Math.PI * 0.32, 11);
    p.text(130, 186, "drag it over", { size: 20, fill: "#8d8b84" });
  }

  function drawMenuBar(p) {
    p.rect(4, 12, 252, 28);
    // wifi, battery, clock
    p.path("M196 32 Q 203 22 210 32", { roughness: 0.6 });
    p.path("M199 34 Q 203 28 207 34", { roughness: 0.6 });
    p.rect(216, 21, 16, 10, { roughness: 0.6 });
    p.line(234, 24, 234, 28, { roughness: 0.4 });
    p.text(18, 33, "file  edit  view", { size: 17, anchor: "start" });
    // cambio icon, highlighted
    p.rect(158, 15, 26, 22, { fill: "rgba(241,239,232,0.16)", fillStyle: "solid", roughness: 0.8 });
    fan(p, 171, 33, 11, 3);
    // dropdown
    p.rect(128, 46, 104, 118, { fill: BG, fillStyle: "solid" });
    const items = ["mp4", "gif", "webm"];
    items.forEach((t, i) => p.text(142, 72 + i * 24, "→ " + t, { size: 21, anchor: "start" }));
    p.line(136, 134, 224, 134, { roughness: 0.6, strokeWidth: 1 });
    p.text(142, 155, "compress", { size: 19, anchor: "start" });
    // the dragged file
    fileIcon(p, 20, 100, 38, 48, "mov");
    p.curve([[60, 104], [104, 68], [154, 36]], { strokeLineDash: [5, 6], strokeWidth: 1.2 });
    arrowHead(p, 156, 35, -Math.PI * 0.22, 10);
    p.text(39, 172, "clip.mov", { size: 20 });
  }

  function drawInstaller(p) {
    p.rect(12, 12, 236, 150);
    p.line(12, 34, 248, 34);
    p.text(24, 29, "cambio setup", { size: 17, anchor: "start" });
    p.text(36, 78, "installing…", { size: 22, anchor: "start" });
    p.rect(36, 92, 188, 16);
    p.rect(36, 92, 120, 16, { fill: INK, fillStyle: "hachure", hachureGap: 4, stroke: "none" });
    p.rect(172, 126, 52, 24);
    p.text(198, 144, "next", { size: 19 });
  }

  function drawTerminal(p) {
    p.rect(12, 12, 236, 150, { fill: "#141416", fillStyle: "solid" });
    p.line(12, 34, 248, 34);
    for (const x of [24, 36, 48]) p.circle(x, 23, 7);
    p.text(26, 66, "$ chmod +x Cambio.AppImage", { size: 11.5, anchor: "start", font: "mono" });
    p.text(26, 90, "$ ./Cambio.AppImage", { size: 11.5, anchor: "start", font: "mono" });
    p.line(26, 108, 33, 108, { strokeWidth: 2, roughness: 0.2 });
    p.text(130, 186, "make it runnable", { size: 20, fill: "#8d8b84" });
  }

  function drawRightClick(p) {
    fileIcon(p, 12, 24, 42, 54, "heic");
    p.rect(62, 40, 192, 128, { fill: BG, fillStyle: "solid" });
    ["open", "copy", "rename"].forEach((t, i) => p.text(78, 66 + i * 22, t, { size: 19, anchor: "start" }));
    p.line(70, 122, 246, 122, { roughness: 0.6, strokeWidth: 1 });
    p.rect(68, 130, 180, 30, { fill: "rgba(241,239,232,0.16)", fillStyle: "solid", roughness: 0.8 });
    p.text(78, 151, "convert with cambio", { size: 19, anchor: "start" });
    // cursor
    p.path("M230 146 L230 168 L236 162 L242 172 L246 170 L240 160 L248 159 Z", {
      fill: INK,
      fillStyle: "solid",
      roughness: 0.5,
    });
  }

  const STEPS = {
    mac: [
      {
        title: "download it",
        body: "grab <code>Cambio.dmg</code> and open it.",
        draw: (p) => drawDownload(p, "Cambio.dmg", "dmg"),
      },
      {
        title: "drag it to applications",
        body: "drop the peacock on the applications folder. done installing.",
        draw: drawDragToApps,
      },
      {
        title: "drop a file on it",
        body: "drag any file onto the peacock in your menu bar and pick a format. the new file lands right next to the old one.",
        draw: drawMenuBar,
      },
    ],
    windows: [
      {
        title: "download it",
        body: "grab <code>cambio-setup.exe</code>.",
        draw: (p) => drawDownload(p, "cambio-setup.exe", "exe"),
      },
      { title: "run it", body: "double-click, hit next. it lives in your system tray.", draw: drawInstaller },
      {
        title: "right-click any file",
        body: "pick <em>convert with cambio</em>, then the format you want.",
        draw: drawRightClick,
      },
    ],
    linux: [
      {
        title: "download it",
        body: "grab <code>Cambio.AppImage</code>.",
        draw: (p) => drawDownload(p, "Cambio.AppImage", "app"),
      },
      { title: "make it runnable", body: "one command in your terminal, then open it.", draw: drawTerminal },
      {
        title: "right-click any file",
        body: "pick <em>convert with cambio</em>, then the format you want.",
        draw: drawRightClick,
      },
    ],
  };

  // --- pencil "draw-on" effect, once, when a sketch scrolls into view -------------

  function drawOn(svg) {
    if (REDUCED || !svg.animate) return;
    const strokes = [...svg.querySelectorAll("path")].filter((el) => el.getAttribute("stroke") !== "none");
    const fills = [...svg.querySelectorAll("path")].filter((el) => el.getAttribute("stroke") === "none");
    const texts = [...svg.querySelectorAll("text")];
    const step = Math.min(28, 1100 / Math.max(strokes.length, 1));

    strokes.forEach((el, i) => {
      const len = el.getTotalLength();
      el.style.strokeDasharray = len;
      el.animate([{ strokeDashoffset: len }, { strokeDashoffset: 0 }], {
        duration: 420,
        delay: i * step,
        easing: "cubic-bezier(0.65, 0, 0.35, 1)",
        fill: "backwards",
      }).finished.then(() => (el.style.strokeDasharray = ""));
    });
    const end = strokes.length * step;
    for (const el of [...fills, ...texts]) {
      el.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: 300,
        delay: end * 0.6,
        easing: "ease-out",
        fill: "backwards",
      });
    }
  }

  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        io.unobserve(e.target);
        drawOn(e.target);
      }
    },
    { rootMargin: "0px 0px -15% 0px" }
  );

  // --- render ----------------------------------------------------------------------

  const stepsEl = document.getElementById("steps");
  const banner = document.getElementById("soon-banner");
  const tabs = [...document.querySelectorAll(".tab")];

  function renderSteps(os, animate) {
    stepsEl.innerHTML = "";
    STEPS[os].forEach((s, i) => {
      const div = document.createElement("div");
      div.className = "step";
      const svg = document.createElementNS(NS, "svg");
      svg.setAttribute("viewBox", "0 0 260 192");
      svg.setAttribute("aria-hidden", "true");
      div.appendChild(svg);
      div.insertAdjacentHTML("beforeend", `<h3><span>${i + 1}.</span>${s.title}</h3><p>${s.body}</p>`);
      stepsEl.appendChild(div);
      s.draw(pen(svg, 11 + i * 5));
      if (animate) io.observe(svg);
    });
    banner.hidden = os === "mac";
  }

  function selectTab(os, animate = false) {
    for (const t of tabs) t.setAttribute("aria-selected", String(t.dataset.os === os));
    renderSteps(os, animate);
  }

  tabs.forEach((t) => t.addEventListener("click", () => selectTab(t.dataset.os)));

  // detect the visitor's os and adjust the hero button + default tab
  const ua = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  const os = ua.includes("win") ? "windows" : ua.includes("linux") && !/android/.test(ua) ? "linux" : "mac";

  if (os !== "mac") {
    const hero = document.getElementById("hero-download");
    hero.setAttribute("aria-disabled", "true");
    hero.removeAttribute("href");
    hero.querySelector("span").textContent = `${os} — coming soon`;
    document.getElementById("os-note").innerHTML =
      'on a mac? <a href="https://github.com/luiginotmario/cambio/releases/latest">download for mac</a>';
  }

  const peacock = document.getElementById("peacock");
  drawPeacock(peacock);
  drawOn(peacock);
  selectTab(os, true);
})();
