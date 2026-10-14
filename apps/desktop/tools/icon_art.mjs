// The Panoply icon drawings, as SVG strings. Used by gen_app_icon.mjs (and icon_sheet.mjs).
//
// One mark everywhere: the Fan. Three rounded cards spread from one pivot like a hand of cards: the
// left card near-black, the right card graphite, the front card upright in the accent blue
// (#2D6CDF). A thin plate-coloured stroke runs around the front card so it reads as sitting on top,
// and a plate-coloured rivet sits on the front card's bottom edge like a hinge pin. Canonical
// geometry on a 100 unit box (UNIT below): cards 26 x 52, radius 8, all rotated about the pivot
// (50, 82) with their bottoms 6 units above it; side cards at 42 degrees, so their lower ends show
// as short feet either side of the hinge. Stroke 2.4, rivet radius 3 at (50, 76). The layer files
// in packaging/icon/fan-layers are exactly this geometry, which is the geometry the app UI embeds.
//
//   macOS    a continuous-corner squircle body on the macOS icon grid, cards with contact shadows
//            and a hairline top highlight, and a soft ambient shadow baked in
//   Windows  a Fluent style tile: nearly full bleed, circular corners, flat, one tight shadow
//   Linux    a GNOME style rounded square (a looser squircle) with its soft shadow baked in
//
// Sizes up to 64 px are not scaled copies of the master: each is drawn on the pixel grid, so the
// front card's edges land on whole pixels. At 32 px and below the spread narrows to 36 degrees and
// the rivet goes; at 16 px (and on the Windows 16 tile) only two cards remain. Nothing glows.

export const BLUE = "#2D6CDF";

const f = (n) => String(Math.round(n * 1000) / 1000);

/**
 * A continuous-corner rounded square (Apple's "squircle") as an SVG path: each corner is a circular
 * arc joined to the straight edges by two cubic Beziers, so curvature ramps up instead of jumping.
 * This is the construction Figma calls corner smoothing; smooth 0.6 with a radius of 22.5 percent of
 * the side is the macOS icon shape. smooth 0 gives plain circular corners.
 */
export function squirclePath(x, y, size, radius, smooth) {
  const h = size / 2;
  const R = radius;
  let s = smooth;
  let p = (1 + s) * R;
  s = Math.min(s, h / R - 1);
  p = Math.min(p, h);
  const rad = (d) => (d * Math.PI) / 180;
  const arcMeasure = 90 * (1 - s);
  const arcLen = Math.sin(rad(arcMeasure / 2)) * R * Math.SQRT2;
  const alpha = (90 - arcMeasure) / 2;
  const p3p4 = R * Math.tan(rad(alpha / 2));
  const beta = 45 * s;
  const c = p3p4 * Math.cos(rad(beta));
  const d = c * Math.tan(rad(beta));
  const b = (p - arcLen - c - d) / 3;
  const a = 2 * b;
  // Top right corner, relative to the centre, then rotated a quarter turn clockwise per corner.
  const S = [h - p, -h];
  const E1 = [S[0] + a + b + c, S[1] + d];
  const E2 = [E1[0] + arcLen, E1[1] + arcLen];
  const corner = {
    start: S,
    c1: [[S[0] + a, S[1]], [S[0] + a + b, S[1]], E1],
    arc: E2,
    c2: [[E2[0] + d, E2[1] + c], [E2[0] + d, E2[1] + b + c], [E2[0] + d, E2[1] + a + b + c]],
  };
  const cx = x + h;
  const cy = y + h;
  const rot = ([px, py], k) => {
    let [qx, qy] = [px, py];
    for (let i = 0; i < k; i++) [qx, qy] = [-qy, qx];
    return `${f(cx + qx)} ${f(cy + qy)}`;
  };
  let out = "";
  for (let k = 0; k < 4; k++) {
    out += `${k === 0 ? "M" : "L"}${rot(corner.start, k)}`;
    out += `C${corner.c1.map((q) => rot(q, k)).join(" ")}`;
    out += `A${f(R)} ${f(R)} 0 0 1 ${rot(corner.arc, k)}`;
    out += `C${corner.c2.map((q) => rot(q, k)).join(" ")}`;
  }
  return `${out}Z`;
}


// Plate shapes per platform, as fractions of the body side.
const SHAPES = {
  mac: { radius: 0.225, smooth: 0.6 },
  win: { radius: 0.18, smooth: 0 },
  linux: { radius: 0.21, smooth: 0.35 },
};

const SHADOW = "#0B1020";

/** The canonical fan on a 100 unit box (see the header). `up` is how far the card bottoms sit below the pivot (negative: above it). */
export const UNIT = { w: 26, h: 52, r: 8, angle: 42, up: -6, pivotY: 82, stroke: 2.4, rivetR: 3, rivetDy: 6 };

const rad = (d) => (d * Math.PI) / 180;

function inRoundRect(px, py, w, h, r) {
  const hx = w / 2;
  const hy = h / 2;
  const x = Math.abs(px);
  const y = Math.abs(py + hy);
  if (x > hx || y > hy) return false;
  const dx = Math.max(x - (hx - r), 0);
  const dy = Math.max(y - (hy - r), 0);
  return dx * dx + dy * dy <= r * r;
}

/** Bounding box of the cards relative to the pivot (x to the right, y down, so the cards are at negative y). */
export function fanExtents({ w, h, r, angle, two, up = 0, cut = null }) {
  const step = Math.max(w, h) / 160;
  let minx = Infinity, maxx = -Infinity, miny = Infinity, maxy = -Infinity;
  const kinds = two ? [-angle, 0] : [-angle, angle, 0];
  for (let x = -h * 1.2; x <= h * 1.2; x += step) {
    for (let y = -h * 1.2; y <= h * 0.5; y += step) {
      for (const a of kinds) {
        if (cut !== null && a !== 0 && y > up - cut) continue; // side cards cut at the front card's bottom line
        const t = -rad(a);
        const lx = x * Math.cos(t) - y * Math.sin(t);
        const ly = x * Math.sin(t) + y * Math.cos(t);
        // Card local frame: x centred, y from -h (top) to 0 (pivot). inRoundRect wants the box centred in y.
        if (inRoundRect(lx, ly - up, w, h, r)) {
          minx = Math.min(minx, x); maxx = Math.max(maxx, x);
          miny = Math.min(miny, y); maxy = Math.max(maxy, y);
        }
      }
    }
  }
  return { minx, maxx, miny, maxy };
}

const rr = (x, y, w, h, r) =>
  `M${f(x + r)} ${f(y)}H${f(x + w - r)}A${f(r)} ${f(r)} 0 0 1 ${f(x + w)} ${f(y + r)}V${f(y + h - r)}A${f(r)} ${f(r)} 0 0 1 ${f(x + w - r)} ${f(y + h)}H${f(x + r)}A${f(r)} ${f(r)} 0 0 1 ${f(x)} ${f(y + h - r)}V${f(y + r)}A${f(r)} ${f(r)} 0 0 1 ${f(x + r)} ${f(y)}Z`;

/** The three cards of a fan config as { kind, x, y, w, h, r, angle } in canvas units. */
function cards(fan) {
  const kinds = fan.two ? [["left", -fan.angle], ["front", 0]] : [["left", -fan.angle], ["right", fan.angle], ["front", 0]];
  return kinds.map(([kind, angle]) => ({ kind, angle, x: fan.cx - fan.w / 2, y: fan.py - fan.h + (fan.up ?? 0), w: fan.w, h: fan.h, r: fan.r }));
}

const cardAttrs = (c, fan, grow = 0) =>
  `x="${f(c.x - grow)}" y="${f(c.y - grow)}" width="${f(c.w + 2 * grow)}" height="${f(c.h + 2 * grow)}" rx="${f(c.r + grow)}"${
    c.angle ? ` transform="rotate(${c.angle} ${f(fan.cx)} ${f(fan.py)})"` : ""
  }`;

/**
 * One drawing. `canvas` is the viewBox side (1024 for a master, the pixel size for a tuned drawing).
 *   body   { x, size, shape }          plate position and side; shape is a key of SHAPES
 *   plate  [top, bottom]               plate colours (a very quiet vertical shift)
 *   edge   hairline on the plate, or null: { w, top, bottom } or { w, flat, flatOp }
 *   fan    { cx, py, w, h, r, angle, stroke, rivet: {dy, r} | null, two }   in canvas units
 *   cols   { left, right, front }      [top, bottom] fills
 *   shadow [{dy, std, op}]             ambient and contact shadows of the whole body
 *   lift   [{dy, std, op}]             the same for every card, onto whatever lies behind it
 *   rim    { w, left, right, front }   hairline top highlight opacities (or null)
 */
export function drawing(cfg) {
  const { canvas: C, body, plate, edge, fan, cols, shadow = [], lift = [], rim } = cfg;
  const shape = SHAPES[body.shape];
  const path = squirclePath(body.x, body.x, body.size, body.size * shape.radius, shape.smooth);
  const grad = (id, [top, bottom], units = "") =>
    `<linearGradient id="${id}" x1="0" y1="${units ? body.x : 0}" x2="0" y2="${units ? body.x + body.size : 1}"${units}><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient>`;
  const blurIds = new Set();
  const blur = (std) => {
    const id = `b${String(std).replace(".", "_")}`;
    blurIds.add([id, std]);
    return id;
  };
  // The plate gradient is in canvas units so the stroke and the rivet match the plate exactly.
  let defs = grad("plate", plate, ' gradientUnits="userSpaceOnUse"');
  for (const k of ["left", "right", "front"]) defs += grad(`c-${k}`, cols[k]);
  if (edge && !edge.flat) {
    defs += `<linearGradient id="edge" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${edge.top[0]}" stop-opacity="${edge.top[1]}"/><stop offset="0.45" stop-color="${edge.top[0]}" stop-opacity="0"/><stop offset="0.55" stop-color="${edge.bottom[0]}" stop-opacity="0"/><stop offset="1" stop-color="${edge.bottom[0]}" stop-opacity="${edge.bottom[1]}"/></linearGradient>`;
  }
  if (edge) defs += `<clipPath id="bodyclip"><path d="${path}"/></clipPath>`;
  if (rim) {
    defs += `<linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF" stop-opacity="1"/><stop offset="0.32" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient>`;
  }
  let shadows = "";
  for (const s of shadow) {
    shadows += `<path d="${path}" transform="translate(0 ${f(s.dy)})" fill="${SHADOW}" fill-opacity="${s.op}" filter="url(#${blur(s.std)})"/>`;
  }
  // Back to front: each card's shadow lands on what is already drawn (the plate, then the cards
  // behind it). The front card's ring (plate coloured) goes under its own shadow.
  // At 24 px and below (fan.cut set) the side cards are cut flat at the front card's bottom line,
  // because feet a pixel or two long only read as specks. From 32 px up the feet stay.
  if (fan.cut != null) defs += `<clipPath id="cut"><rect x="0" y="0" width="${C}" height="${f(fan.py + fan.up - fan.cut)}"/></clipPath>`;
  let layers = "";
  for (const c of cards(fan)) {
    const wrap = (inner) => (fan.cut != null && c.kind !== "front" ? `<g clip-path="url(#cut)">${inner}</g>` : inner);
    if (c.kind === "front" && fan.stroke > 0) layers += `<rect ${cardAttrs(c, fan, fan.stroke)} fill="url(#plate)"/>`;
    for (const s of lift) {
      layers += `<g transform="translate(0 ${f(s.dy)})" fill="${SHADOW}" fill-opacity="${s.op}" filter="url(#${blur(s.std)})">${wrap(`<rect ${cardAttrs(c, fan)}/>`)}</g>`;
    }
    layers += wrap(`<rect ${cardAttrs(c, fan)} fill="url(#c-${c.kind})"/>`);
    if (rim && rim[c.kind] > 0) {
      const w = rim.w;
      layers += wrap(`<rect ${cardAttrs({ ...c, x: c.x + w / 2, y: c.y + w / 2, w: c.w - w, h: c.h - w, r: c.r - w / 2 }, fan)} fill="none" stroke="url(#rim)" stroke-opacity="${rim[c.kind]}" stroke-width="${f(w)}"/>`);
    }
  }
  if (fan.rivet) layers += `<circle cx="${f(fan.cx)}" cy="${f(fan.py - fan.rivet.dy)}" r="${f(fan.rivet.r)}" fill="url(#plate)"/>`;
  for (const [id, std] of blurIds) {
    defs += `<filter id="${id}" filterUnits="userSpaceOnUse" x="0" y="0" width="${C}" height="${C}" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${std}"/></filter>`;
  }
  const edgePath = edge
    ? `<path d="${path}" fill="none" stroke="${edge.flat ?? "url(#edge)"}"${edge.flat ? ` stroke-opacity="${edge.flatOp}"` : ""} stroke-width="${f(edge.w * 2)}" clip-path="url(#bodyclip)"/>`
    : "";
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${C} ${C}" width="${C}" height="${C}">
<title>Panoply</title>
<defs>${defs}</defs>
${shadows}<path d="${path}" fill="url(#plate)"/>${edgePath}${layers}
</svg>
`;
}

// ---------------------------------------------------------------------------------------------
// Colours
const ACCENT = ["#3D80F2", "#2760D3"];
const LIGHT_COLS = { left: ["#2A2E36", "#1D2026"], right: ["#42464F", "#32363E"], front: ACCENT };
const FLAT_COLS = { left: ["#23262D", "#23262D"], right: ["#3A3E47", "#3A3E47"], front: [BLUE, BLUE] };
const DARK_COLS = { left: ["#EEF0F4", "#DEE1E8"], right: ["#B9BDC8", "#A3A8B5"], front: ACCENT };
const WIN_COLS = { left: ["#272A32", "#20232A"], right: ["#40444D", "#363A42"], front: ["#3777E6", "#2C68D6"] };

/** The unit geometry placed on a canvas: its bounding box is centred on (cx, cy) and is `width` wide. */
function masterFan(cx, cy, width) {
  const U = UNIT;
  const e = fanExtents(U);
  const s = width / (e.maxx - e.minx);
  const ucy = U.pivotY + (e.miny + e.maxy) / 2;
  return {
    cx,
    py: cy + (U.pivotY - ucy) * s,
    w: U.w * s,
    h: U.h * s,
    r: U.r * s,
    angle: U.angle,
    up: U.up * s,
    stroke: U.stroke * s,
    rivet: { dy: U.rivetDy * s, r: U.rivetR * s },
  };
}

// Pixel-grid fans by canvas size. w and h are whole pixels with w even, so with the fan centred on a
// whole pixel the front card's edges land on pixel boundaries.
const FAN_TUNED = {
  16: { w: 6, h: 10, r: 1.5, angle: 32, stroke: 1, two: true },
  20: { w: 6, h: 9, r: 1.5, angle: 32, stroke: 1 },
  22: { w: 6, h: 10, r: 1.5, angle: 32, stroke: 1 },
  24: { w: 6, h: 10, r: 1.5, angle: 32, stroke: 1 },
  32: { w: 8, h: 13, r: 2, angle: 32, stroke: 1 },
  40: { w: 8, h: 15, r: 2, angle: 36, stroke: 1 },
  48: { w: 10, h: 16, r: 2.5, angle: 38, stroke: 1, rivet: { r: 1 } },
  64: { w: 12, h: 20, r: 3, angle: 40, stroke: 1.5, rivet: { r: 1.5 } },
};

/** Centre the tuned fan on a whole pixel inside a box centred at (bx, by). */
function tunedFan(px, bx, by) {
  const t = FAN_TUNED[px];
  const up = -Math.max(1, Math.round(t.h * 0.115));
  const cut = px <= 24 ? 0 : null;
  const e = fanExtents({ ...t, up, cut });
  return {
    cut,
    ...t,
    up,
    cx: Math.round(bx - (e.minx + e.maxx) / 2),
    py: Math.round(by - (e.miny + e.maxy) / 2),
    rivet: t.rivet ? { dy: -up, r: t.rivet.r } : null,
  };
}

function effectsFor(px) {
  // Shadows and rims arrive with size: none at 24 and below, a whisper at 32, full at 48 and 64.
  if (px <= 24) return {};
  if (px <= 32) {
    return { shadow: [{ dy: 0.8, std: 0.7, op: 0.2 }], lift: [{ dy: 0.6, std: 0.5, op: 0.2 }] };
  }
  if (px <= 48) {
    return {
      shadow: [{ dy: 1.5, std: 1.5, op: 0.2 }],
      lift: [{ dy: 1, std: 1, op: 0.24 }],
      rim: { w: 1, left: 0.18, right: 0.2, front: 0.3 },
    };
  }
  return {
    shadow: [{ dy: 2, std: 1.8, op: 0.2 }],
    lift: [{ dy: 1.5, std: 1.5, op: 0.24 }],
    rim: { w: 1, left: 0.18, right: 0.2, front: 0.3 },
  };
}

// ---------------------------------------------------------------------------------------------
// macOS

/** The macOS master, 1024 canvas, body 824 with 100 px of margin (Apple's grid). */
export function macMaster(dark = false) {
  return drawing({
    canvas: 1024,
    body: { x: 100, size: 824, shape: "mac" },
    plate: dark ? ["#2D3039", "#1B1D23"] : ["#FDFDFE", "#EFF0F4"],
    edge: dark
      ? { w: 2, top: ["#FFFFFF", 0.16], bottom: ["#000000", 0.3] }
      : { w: 2, top: ["#FFFFFF", 0.9], bottom: ["#000000", 0.07] },
    // The fan is 528 wide (64 percent of the 824 body). Nudged 4 right (the near-black left card is
    // the heavier one) and 5 up (the shadows add weight below).
    fan: masterFan(516, 507, 528),
    cols: dark ? DARK_COLS : LIGHT_COLS,
    shadow: dark
      ? [{ dy: 12, std: 15, op: 0.5 }, { dy: 2, std: 2.5, op: 0.4 }]
      : [{ dy: 12, std: 15, op: 0.22 }, { dy: 2, std: 2.5, op: 0.22 }],
    lift: dark
      ? [{ dy: 16, std: 15, op: 0.3 }, { dy: 5, std: 3.5, op: 0.36 }]
      : [{ dy: 16, std: 16, op: 0.16 }, { dy: 5, std: 3.5, op: 0.32 }],
    rim: dark ? { w: 1.5, left: 0, right: 0, front: 0.34 } : { w: 1.5, left: 0.2, right: 0.22, front: 0.42 },
  });
}

const MAC_TUNED = {
  16: { bx: 1, bs: 14 },
  20: { bx: 1, bs: 18 },
  24: { bx: 2, bs: 20 },
  32: { bx: 2, bs: 28 },
  48: { bx: 3, bs: 42 },
  64: { bx: 4, bs: 56 },
};

export function macTuned(px) {
  const t = MAC_TUNED[px];
  const c = t.bx + t.bs / 2;
  return drawing({
    canvas: px,
    body: { x: t.bx, size: t.bs, shape: "mac" },
    plate: px <= 24 ? ["#F6F7FA", "#E4E7ED"] : ["#FDFDFE", "#EBEDF2"],
    edge: px >= 48 ? { w: 1, top: ["#FFFFFF", 0.9], bottom: ["#000000", 0.08] } : null,
    fan: tunedFan(px, c, c),
    cols: px <= 24 ? FLAT_COLS : LIGHT_COLS,
    ...effectsFor(px),
  });
}

// ---------------------------------------------------------------------------------------------
// Windows (Fluent)

export function winMaster() {
  return drawing({
    canvas: 1024,
    body: { x: 32, size: 960, shape: "win" },
    plate: ["#FFFFFF", "#EEF0F5"],
    edge: { w: 3, flat: "#000000", flatOp: 0.1 },
    fan: masterFan(516, 508, 614),
    cols: WIN_COLS,
    shadow: [{ dy: 8, std: 8, op: 0.14 }],
    lift: [{ dy: 6, std: 6, op: 0.18 }],
  });
}

// Windows tiles are full bleed: the plate is the whole canvas. No tuned 22 on Windows.
export function winTuned(px) {
  const c = px / 2;
  return drawing({
    canvas: px,
    body: { x: 0, size: px, shape: "win" },
    plate: ["#FFFFFF", "#EDEFF4"],
    edge: { w: px <= 24 ? 0.5 : 1, flat: "#000000", flatOp: px <= 24 ? 0.14 : 0.1 },
    fan: tunedFan(px, c, c),
    cols: px <= 24 ? FLAT_COLS : WIN_COLS,
    lift: px >= 40 ? [{ dy: px / 170, std: px / 170, op: 0.18 }] : [],
  });
}

// ---------------------------------------------------------------------------------------------
// Linux (GNOME)

export function linuxMaster() {
  return drawing({
    canvas: 1024,
    body: { x: 64, size: 896, shape: "linux" },
    plate: ["#FAFAFC", "#E6E9EF"],
    edge: { w: 2, flat: "#000000", flatOp: 0.12 },
    fan: masterFan(516, 507, 573),
    cols: LIGHT_COLS,
    shadow: [{ dy: 12, std: 14, op: 0.2 }, { dy: 3, std: 2.5, op: 0.2 }],
    lift: [{ dy: 10, std: 10, op: 0.14 }, { dy: 4, std: 3, op: 0.28 }],
    rim: { w: 1.5, left: 0.18, right: 0.2, front: 0.36 },
  });
}

const LINUX_TUNED = {
  16: { bx: 1, bs: 14 },
  22: { bx: 1, bs: 20 },
  24: { bx: 1, bs: 22 },
  32: { bx: 2, bs: 28 },
  48: { bx: 3, bs: 42 },
  64: { bx: 4, bs: 56 },
};

export function linuxTuned(px) {
  const t = LINUX_TUNED[px];
  const c = t.bx + t.bs / 2;
  return drawing({
    canvas: px,
    body: { x: t.bx, size: t.bs, shape: "linux" },
    plate: px <= 24 ? ["#F6F7FA", "#E4E7ED"] : ["#FAFAFC", "#E6E9EF"],
    edge: px <= 24 ? null : { w: 1, flat: "#000000", flatOp: 0.12 },
    fan: tunedFan(px, c, c),
    cols: px <= 24 ? FLAT_COLS : LIGHT_COLS,
    ...effectsFor(px),
  });
}

export const TUNED_SIZES = {
  mac: Object.keys(MAC_TUNED).map(Number),
  win: [16, 20, 24, 32, 40, 48, 64],
  linux: Object.keys(LINUX_TUNED).map(Number),
};

// ---------------------------------------------------------------------------------------------
// Shared flat geometry and the README brand assets

/** The five flat layers of the canonical fan, 100 x 100 viewBox, single colour (currentColor). */
export function fanLayers() {
  const U = UNIT;
  const x = 50 - U.w / 2;
  const y = U.pivotY - U.h + U.up;
  const svg = (inner) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="currentColor">${inner}</svg>\n`;
  const card = (angle) =>
    `<rect x="${x}" y="${y}" width="${U.w}" height="${U.h}" rx="${U.r}"${angle ? ` transform="rotate(${angle} 50 ${U.pivotY})"` : ""}/>`;
  const g = U.stroke;
  return {
    "left.svg": svg(card(-U.angle)),
    "right.svg": svg(card(U.angle)),
    "front.svg": svg(card(0)),
    "front-stroke.svg": svg(
      `<path fill-rule="evenodd" d="${rr(x - g, y - g, U.w + 2 * g, U.h + 2 * g, U.r + g)}${rr(x, y, U.w, U.h, U.r)}"/>`,
    ),
    "rivet.svg": svg(`<circle cx="50" cy="${U.pivotY - U.rivetDy}" r="${U.rivetR}"/>`),
  };
}

/**
 * The flat fan with a transparent gap around the front card (a mask), for the README marks.
 * Scaled so the fan is exactly `width` wide with its left edge at x0, centred vertically on cy.
 * Returns the SVG fragment (defs, cards) without a plate, stroke ring or rivet.
 */
function flatFan({ x0, cy, width, left, right, front, id }) {
  const U = UNIT;
  const e = fanExtents(U);
  const s = width / (e.maxx - e.minx);
  const cx = x0 + (-e.minx) * s;
  const py = cy - ((e.miny + e.maxy) / 2) * s;
  const fan = { cx, py, w: U.w * s, h: U.h * s, r: U.r * s, angle: U.angle, up: U.up * s };
  const [l, r, fr] = cards({ ...fan, two: false });
  const gap = U.stroke * s;
  const box = `x="${f(x0 - 10)}" y="${f(cy - width)}" width="${f(width + 20)}" height="${f(width * 2)}"`;
  return (
    `<defs><mask id="${id}" maskUnits="userSpaceOnUse" ${box}><rect ${box} fill="#fff"/><rect ${cardAttrs(fr, fan, gap)} fill="#000"/></mask></defs>` +
    `<g mask="url(#${id})"><rect ${cardAttrs(l, fan)} fill="${left}"/><rect ${cardAttrs(r, fan)} fill="${right}"/></g>` +
    `<rect ${cardAttrs(fr, fan)} fill="${front}"/>`
  );
}

const MARK_COLORS = {
  light: { left: "#23262D", right: "#3A3E47", front: BLUE },
  dark: { left: "#E6E8EE", right: "#AEB3BF", front: "#4C87F2" },
};

/** docs/assets/panoply-mark.svg and panoply-mark-dark.svg: the fan alone on a transparent 544 box. */
export function markSvg(dark = false) {
  const c = MARK_COLORS[dark ? "dark" : "light"];
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 544 544" fill="none"><title>Panoply</title>${flatFan({ x0: 0, cy: 272, width: 544, ...c, id: "gap" })}</svg>\n`;
}

/** The fan as it replaces the four squares at the start of the logo files (same 544 box, same left edge). */
export function logoFan(dark = false) {
  const c = MARK_COLORS[dark ? "dark" : "light"];
  return flatFan({ x0: 0, cy: 272, width: 544, ...c, id: "gap" });
}
