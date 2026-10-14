// The Panoply icon drawings, as SVG strings. Used by gen_app_icon.mjs (and icon_sheet.mjs).
//
// One mark everywhere: the Fan. Three equal tool cards fanned from one pivot like a hand of cards,
// the front one upright in the accent blue (#2D6CDF) and the two behind it a calm mid graphite,
// each card a continuous-corner rounded rectangle from the plate's own corner family. Canonical
// geometry on a 100 unit box (UNIT below): cards 30 x 54 (0.56 wide to tall), radius 7.5, the side
// cards at 20 degrees either side, all turned about a pivot 24 units below the card bottoms, so the
// three card tops step down and out in a row and the bottoms gather toward the pivot. There is no ring
// around the front card and no hinge pin: the blue against the graphite and a soft contact shadow
// are what separate the layers. The layer files in packaging/icon/fan-layers are exactly this
// geometry, which is the geometry the app UI embeds.
//
//   macOS    a continuous-corner squircle body on the macOS icon grid, cards with contact shadows
//            and a hairline top highlight, and a soft ambient shadow baked in
//   Windows  a Fluent style tile: nearly full bleed, circular corners, flat, one tight shadow
//   Linux    a GNOME style rounded square (a looser squircle) with its soft shadow baked in
//
// Sizes up to 64 px are not scaled copies of the master: each is drawn on the pixel grid, so the
// front card's edges land on whole pixels and the side cards keep a clear step at the top. At 16 px
// (and on the Windows 16 tile) only two cards remain. Nothing glows.

export const BLUE = "#2D6CDF";

const f = (n) => String(Math.round(n * 1000) / 1000);

/**
 * A continuous-corner rounded square (Apple's "squircle") as an SVG path: each corner is a circular
 * arc joined to the straight edges by two cubic Beziers, so curvature ramps up instead of jumping.
 * This is the construction Figma calls corner smoothing; smooth 0.6 with a radius of 22.5 percent of
 * the side is the macOS icon shape. smooth 0 gives plain circular corners.
 */
export function squirclePath(x, y, size, radius, smooth) {
  return squirclePathRect(x, y, size, size, radius, smooth);
}

/** The same construction for a w x h rectangle (the cards use it, so they share the plate's corner family). */
export function squirclePathRect(x, y, w, hgt, radius, smooth) {
  const hw = w / 2;
  const hh = hgt / 2;
  const half = Math.min(hw, hh);
  const R = radius;
  let s = smooth;
  let p = (1 + s) * R;
  s = Math.min(s, half / R - 1);
  p = Math.min(p, half);
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
  // Top right corner as offsets from its vertex, turned a quarter clockwise for each following corner.
  const S = [-p, 0];
  const E1 = [S[0] + a + b + c, S[1] + d];
  const E2 = [E1[0] + arcLen, E1[1] + arcLen];
  const corner = {
    start: S,
    c1: [[S[0] + a, S[1]], [S[0] + a + b, S[1]], E1],
    arc: E2,
    c2: [[E2[0] + d, E2[1] + c], [E2[0] + d, E2[1] + b + c], [E2[0] + d, E2[1] + a + b + c]],
  };
  const cx = x + hw;
  const cy = y + hh;
  const verts = [[hw, -hh], [hw, hh], [-hw, hh], [-hw, -hh]];
  const at = ([px, py], k) => {
    let [qx, qy] = [px, py];
    for (let i = 0; i < k; i++) [qx, qy] = [-qy, qx];
    return `${f(cx + verts[k][0] + qx)} ${f(cy + verts[k][1] + qy)}`;
  };
  let out = "";
  for (let k = 0; k < 4; k++) {
    out += `${k === 0 ? "M" : "L"}${at(corner.start, k)}`;
    out += `C${corner.c1.map((q) => at(q, k)).join(" ")}`;
    out += `A${f(R)} ${f(R)} 0 0 1 ${at(corner.arc, k)}`;
    out += `C${corner.c2.map((q) => at(q, k)).join(" ")}`;
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

/** The canonical fan on a 100 unit box (see the header). `up` is how far the card bottoms sit below the pivot (negative: above it). `span` is the fan's width as a fraction of the plate body. */
export const UNIT = { w: 30, h: 54, r: 6.5, angle: 20, up: -24, pivotY: 98.7, span: 0.72, smooth: 0.35 };

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
export function fanExtents({ w, h, r, angle, two, up = 0, flat = false }) {
  const step = Math.max(w, h) / 160;
  let minx = Infinity, maxx = -Infinity, miny = Infinity, maxy = -Infinity;
  const kinds = two ? [-angle, 0] : [-angle, angle, 0];
  const reach = (h - up) * 1.2; // the card tops are h - up above the pivot
  for (let x = -reach; x <= reach; x += step) {
    for (let y = -reach; y <= h * 0.5; y += step) {
      for (const a of kinds) {
        if (flat && a !== 0 && y > up) continue; // side cards cut flat at the front card's bottom line
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

/** The three cards of a fan config as { kind, x, y, w, h, r, angle } in canvas units. */
function cards(fan) {
  const kinds = fan.two ? [["left", -fan.angle], ["front", 0]] : [["left", -fan.angle], ["right", fan.angle], ["front", 0]];
  return kinds.map(([kind, angle]) => ({ kind, angle, x: fan.cx - fan.w / 2, y: fan.py - fan.h + (fan.up ?? 0), w: fan.w, h: fan.h, r: fan.r }));
}

/** One card as a path (rotated about the pivot). `grow` offsets the outline outwards (the ring of the small sizes). */
const cardEl = (c, fan, attrs = "", grow = 0) => {
  const d = squirclePathRect(c.x - grow, c.y - grow, c.w + 2 * grow, c.h + 2 * grow, c.r + grow, fan.smooth ?? 0);
  return `<path d="${d}"${c.angle ? ` transform="rotate(${c.angle} ${f(fan.cx)} ${f(fan.py)})"` : ""}${attrs}/>`;
};

/**
 * One drawing. `canvas` is the viewBox side (1024 for a master, the pixel size for a tuned drawing).
 *   body   { x, size, shape }          plate position and side; shape is a key of SHAPES
 *   plate  [top, bottom]               plate colours (a very quiet vertical shift)
 *   edge   hairline on the plate, or null: { w, top, bottom } or { w, flat, flatOp }
 *   fan    { cx, py, w, h, r, angle, up, smooth, two, flat, ring }   in canvas units
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
  // The plate gradient is in canvas units, so the ring around the front card matches the plate exactly.
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
  // behind it), which is what makes the back cards read as layers.
  // At 32 px and below (fan.flat) the side cards are cut flat at the front card's bottom line: feet a
  // pixel or two long only read as specks.
  if (fan.flat) defs += `<clipPath id="flat"><rect x="0" y="0" width="${C}" height="${f(fan.py + fan.up)}"/></clipPath>`;
  let layers = "";
  for (const c of cards(fan)) {
    const wrap = (inner) => (fan.flat && c.kind !== "front" ? `<g clip-path="url(#flat)">${inner}</g>` : inner);
    if (c.kind === "front" && fan.ring) layers += cardEl(c, fan, ` fill="url(#plate)" fill-opacity="${fan.ringOp ?? 1}"`, fan.ring);
    for (const s of lift) {
      layers += `<g transform="translate(0 ${f(s.dy)})" fill="${SHADOW}" fill-opacity="${s.op}" filter="url(#${blur(s.std)})">${wrap(cardEl(c, fan))}</g>`;
    }
    layers += wrap(cardEl(c, fan, ` fill="url(#c-${c.kind})"`));
    if (rim && rim[c.kind] > 0) {
      const w = rim.w;
      layers += wrap(cardEl({ ...c, x: c.x + w / 2, y: c.y + w / 2, w: c.w - w, h: c.h - w, r: c.r - w / 2 }, fan, ` fill="none" stroke="url(#rim)" stroke-opacity="${rim[c.kind]}" stroke-width="${f(w)}"`));
    }
  }
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
// The back cards are a calm mid graphite with a quiet gradient, the right one a step lighter, so the
// silhouette is a hand of cards and not a dark V. On the dark plate they are light graphite.
export const LIGHT_COLS = { left: ["#7C828E", "#686E7A"], right: ["#8A909C", "#767C88"], front: ACCENT };
export const FLAT_COLS = { left: ["#727884", "#727884"], right: ["#808692", "#808692"], front: [BLUE, BLUE] };
export const DARK_COLS = { left: ["#8A90A0", "#767C8A"], right: ["#A0A5B2", "#8C92A0"], front: ACCENT };
export const WIN_COLS = { left: ["#7A808C", "#666C78"], right: ["#888E9A", "#747A86"], front: ["#3777E6", "#2C68D6"] };

/** The unit geometry placed on a canvas: its bounding box is centred on (cx, cy) and is `width` wide. */
export function masterFan(cx, cy, width) {
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
    smooth: U.smooth,
  };
}

// Pixel-grid fans by canvas size. w and h are whole pixels with w even, so with the fan centred on a
// whole pixel the front card's edges land on pixel boundaries. `up` is the pivot's depth below the
// card bottoms, in pixels. The step between the card tops stays at 20 degrees; at 16 px two cards remain.
const FAN_TUNED = {
  16: { flat: true, w: 4, h: 9, r: 1, angle: 20, up: -4, two: true },
  20: { flat: true, ring: 1, ringOp: 0.5, w: 6, h: 10, r: 1.5, angle: 20, up: -4 },
  22: { flat: true, ring: 1, ringOp: 0.5, w: 6, h: 10, r: 1.5, angle: 20, up: -4 },
  24: { flat: true, ring: 1, ringOp: 0.5, w: 6, h: 10, r: 1.5, angle: 20, up: -4 },
  32: { flat: true, ring: 1, ringOp: 0.5, w: 8, h: 14, r: 2, angle: 20, up: -6 },
  40: { w: 10, h: 18, r: 2.5, angle: 20, up: -8 },
  48: { w: 10, h: 18, r: 2.5, angle: 20, up: -8 },
  64: { w: 14, h: 25, r: 3.5, angle: 20, up: -11 },
};

// The Windows tiles are full bleed, so a few sizes carry a larger fan than the inset macOS and Linux plates.
const FAN_TUNED_WIN = {
  16: { flat: true, w: 6, h: 10, r: 1.5, angle: 20, up: -3, two: true },
  48: { w: 12, h: 21, r: 3, angle: 20, up: -9 },
  64: { w: 16, h: 28, r: 4, angle: 20, up: -12 },
};

/** Centre the tuned fan on a whole pixel inside a box centred at (bx, by). */
function tunedFan(px, bx, by, win = false) {
  const t = (win && FAN_TUNED_WIN[px]) || FAN_TUNED[px];
  const e = fanExtents(t);
  return {
    ...t,
    cx: Math.round(bx - (e.minx + e.maxx) / 2),
    py: Math.round(by - (e.miny + e.maxy) / 2),
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
    // The fan is 70 percent of the 824 body wide, centred on its bounding box.
    fan: masterFan(512, 512, 824 * UNIT.span),
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
    fan: masterFan(512, 512, 960 * UNIT.span),
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
    fan: tunedFan(px, c, c, true),
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
    fan: masterFan(512, 512, 896 * UNIT.span),
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

/** Width of the faint plate-coloured seam around the front card in the app UI's small marks, in 100 unit box units. */
const SEAM = 2.6;

/**
 * The four flat layers of the canonical fan, 100 x 100 viewBox, single colour (currentColor): the two
 * back cards, the seam (a ring around the front card, drawn in the plate colour at half strength
 * below 40 pt, as the pixel-grid icon drawings do) and the front card.
 */
export function fanLayers() {
  const fan = { cx: 50, py: UNIT.pivotY, w: UNIT.w, h: UNIT.h, r: UNIT.r, angle: UNIT.angle, up: UNIT.up, smooth: UNIT.smooth };
  const [l, r, fr] = cards(fan);
  const svg = (inner) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="currentColor">${inner}</svg>\n`;
  return {
    "left.svg": svg(cardEl(l, fan)),
    "right.svg": svg(cardEl(r, fan)),
    "front-seam.svg": svg(cardEl(fr, fan, "", SEAM)),
    "front.svg": svg(cardEl(fr, fan)),
  };
}

/**
 * The flat fan for the README marks, three plain cards with no plate. Scaled so the fan is exactly
 * `width` wide with its left edge at x0, centred vertically on cy. Returns the SVG fragment.
 */
function flatFan({ x0, cy, width, left, right, front }) {
  const U = UNIT;
  const e = fanExtents(U);
  const s = width / (e.maxx - e.minx);
  const cx = x0 + (-e.minx) * s;
  const py = cy - ((e.miny + e.maxy) / 2) * s;
  const fan = { cx, py, w: U.w * s, h: U.h * s, r: U.r * s, angle: U.angle, up: U.up * s, smooth: U.smooth };
  const [l, r, fr] = cards(fan);
  return cardEl(l, fan, ` fill="${left}"`) + cardEl(r, fan, ` fill="${right}"`) + cardEl(fr, fan, ` fill="${front}"`);
}

const MARK_COLORS = {
  light: { left: "#727884", right: "#808692", front: BLUE },
  dark: { left: "#808695", right: "#969BA9", front: "#4C87F2" },
};

/** docs/assets/panoply-mark.svg and panoply-mark-dark.svg: the fan alone on a transparent 544 box. */
export function markSvg(dark = false) {
  const c = MARK_COLORS[dark ? "dark" : "light"];
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 544 544" fill="none"><title>Panoply</title>${flatFan({ x0: 0, cy: 272, width: 544, ...c })}</svg>\n`;
}

/** The fan as it replaces the four squares at the start of the logo files (same 544 box, same left edge). */
export function logoFan(dark = false) {
  const c = MARK_COLORS[dark ? "dark" : "light"];
  return flatFan({ x0: 0, cy: 272, width: 544, ...c });
}
