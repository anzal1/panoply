// The Panoply icon drawings, as SVG strings. Used by gen_app_icon.mjs (and icon_sheet.mjs).
//
// One mark everywhere: the four-square Panoply mark (docs/assets/panoply-mark.svg). A 2x2 grid of
// rounded squares, one accent (#2D6CDF, top left) and three ink squares. The proportions of the
// master mark are square 246, gap 52, radius 66 on 544 (square : gap : radius is about 1 : 0.21 : 0.27).
// Each platform keeps those ratios on its own plate:
//
//   macOS    a continuous-corner squircle body on the macOS icon grid, a raised mark with a contact
//            shadow and a hairline rim, and a soft ambient shadow baked in
//   Windows  a Fluent style tile: nearly full bleed, circular corners, flat, one tight shadow
//   Linux    a GNOME style rounded square (a looser squircle) with its soft shadow baked in
//
// Small sizes (64 px and below) are not scaled copies of the master: each one is drawn on the pixel
// grid so every square edge lands on a whole pixel, with no shadow or rim at 24 px and below.

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

/**
 * One drawing. `canvas` is the viewBox side (1024 for a master, the pixel size for a tuned drawing).
 *   body   { x, size, shape }          plate position and side; shape is a key of SHAPES
 *   plate  [top, bottom]               plate colours (a very quiet vertical shift)
 *   edge   { w, top, bottom }          inside hairline on the plate (colour with opacity), or null
 *   mark   { x, y, sq, gap, r }        top left of the grid, square side, gap, corner radius
 *   fills  { accent, ink }             [top, bottom] per square kind
 *   shadow [{dy, std, op}]             ambient and contact shadows of the whole body
 *   lift   [{dy, std, op}]             the same for the raised squares
 *   rim    { w, accent, ink }          hairline top highlight on the squares, opacities (or null)
 */
export function drawing(cfg) {
  const { canvas: C, body, plate, edge, mark, fills, shadow = [], lift = [], rim } = cfg;
  const shape = SHAPES[body.shape];
  const path = squirclePath(body.x, body.x, body.size, body.size * shape.radius, shape.smooth);
  const sqs = [];
  for (const [col, row, kind] of [[0, 0, "accent"], [1, 0, "ink"], [0, 1, "ink"], [1, 1, "ink"]]) {
    sqs.push({ x: mark.x + col * (mark.sq + mark.gap), y: mark.y + row * (mark.sq + mark.gap), kind });
  }
  const grad = (id, [top, bottom]) =>
    `<linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient>`;
  const blurIds = new Set();
  const blur = (std) => {
    const id = `b${String(std).replace(".", "_")}`;
    blurIds.add([id, std]);
    return id;
  };
  let defs = grad("plate", plate) + grad("accent", fills.accent) + grad("ink", fills.ink);
  if (edge && !edge.flat) {
    defs += `<linearGradient id="edge" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${edge.top[0]}" stop-opacity="${edge.top[1]}"/><stop offset="0.45" stop-color="${edge.top[0]}" stop-opacity="0"/><stop offset="0.55" stop-color="${edge.bottom[0]}" stop-opacity="0"/><stop offset="1" stop-color="${edge.bottom[0]}" stop-opacity="${edge.bottom[1]}"/></linearGradient>`;
  }
  if (edge) {
    defs += `<clipPath id="bodyclip"><path d="${path}"/></clipPath>`;
  }
  if (rim) {
    defs += `<linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF" stop-opacity="1"/><stop offset="0.32" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient>`;
  }
  let shadows = "";
  for (const s of shadow) {
    shadows += `<path d="${path}" transform="translate(0 ${f(s.dy)})" fill="${SHADOW}" fill-opacity="${s.op}" filter="url(#${blur(s.std)})"/>`;
  }
  let lifts = "";
  for (const s of lift) {
    lifts += `<g transform="translate(0 ${f(s.dy)})" fill="${SHADOW}" fill-opacity="${s.op}" filter="url(#${blur(s.std)})">${sqs
      .map((q) => `<rect x="${f(q.x)}" y="${f(q.y)}" width="${f(mark.sq)}" height="${f(mark.sq)}" rx="${f(mark.r)}"/>`)
      .join("")}</g>`;
  }
  for (const [id, std] of blurIds) {
    defs += `<filter id="${id}" filterUnits="userSpaceOnUse" x="0" y="0" width="${C}" height="${C}" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${std}"/></filter>`;
  }
  const squares = sqs
    .map((q) => `<rect x="${f(q.x)}" y="${f(q.y)}" width="${f(mark.sq)}" height="${f(mark.sq)}" rx="${f(mark.r)}" fill="url(#${q.kind})"/>`)
    .join("");
  let rims = "";
  if (rim) {
    const w = rim.w;
    rims = sqs
      .map(
        (q) =>
          `<rect x="${f(q.x + w / 2)}" y="${f(q.y + w / 2)}" width="${f(mark.sq - w)}" height="${f(mark.sq - w)}" rx="${f(mark.r - w / 2)}" fill="none" stroke="url(#rim)" stroke-opacity="${rim[q.kind]}" stroke-width="${f(w)}"/>`,
      )
      .join("");
  }
  const edgePath = edge
    ? `<path d="${path}" fill="none" stroke="${edge.flat ?? "url(#edge)"}"${edge.flat ? ` stroke-opacity="${edge.flatOp}"` : ""} stroke-width="${f(edge.w * 2)}" clip-path="url(#bodyclip)"/>`
    : "";
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${C} ${C}" width="${C}" height="${C}">
<title>Panoply</title>
<defs>${defs}</defs>
${shadows}<path d="${path}" fill="url(#plate)"/>${edgePath}${lifts}${squares}${rims}
</svg>
`;
}

// ---------------------------------------------------------------------------------------------
// Colours
const INK = ["#2F333C", "#20232A"];
const INK_FLAT = ["#23262E", "#23262E"];
const ACCENT = ["#3D80F2", "#2760D3"];
const ACCENT_FLAT = [BLUE, BLUE];
const INK_ON_DARK = ["#F4F6FA", "#DCE0E8"];
const ACCENT_ON_DARK = ["#5B95F6", "#4581EC"];

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
    mark: { x: 274, y: 272, sq: 212, gap: 44, r: 57 },
    fills: dark ? { accent: ACCENT_ON_DARK, ink: INK_ON_DARK } : { accent: ACCENT, ink: INK },
    shadow: dark
      ? [{ dy: 12, std: 15, op: 0.5 }, { dy: 2, std: 2.5, op: 0.4 }]
      : [{ dy: 12, std: 15, op: 0.22 }, { dy: 2, std: 2.5, op: 0.22 }],
    lift: dark
      ? [{ dy: 16, std: 15, op: 0.35 }, { dy: 4, std: 3, op: 0.4 }]
      : [{ dy: 18, std: 16, op: 0.2 }, { dy: 5, std: 3.5, op: 0.36 }],
    rim: dark ? { w: 1.5, accent: 0.34, ink: 0 } : { w: 1.5, accent: 0.42, ink: 0.2 },
  });
}

// Pixel-grid tuning. bx/bs: body offset and side; mx: mark offset; sq and gap in whole pixels.
const MAC_TUNED = {
  16: { bx: 1, bs: 14, mx: 2, sq: 5, gap: 2 },
  20: { bx: 1, bs: 18, mx: 3, sq: 6, gap: 2 },
  24: { bx: 2, bs: 20, mx: 5, sq: 6, gap: 2 },
  32: { bx: 2, bs: 28, mx: 6, sq: 9, gap: 2 },
  48: { bx: 3, bs: 42, mx: 9, sq: 13, gap: 4 },
  64: { bx: 4, bs: 56, mx: 12, sq: 18, gap: 4 },
};

function effectsFor(px, k) {
  // Shadows and rims arrive with size: none at 24 and below, a whisper at 32, full at 48 and 64.
  if (px <= 24) return {};
  if (px === 32) {
    return {
      shadow: [{ dy: 0.8, std: 0.7, op: 0.2 * k.shadow }],
      lift: [{ dy: 0.6, std: 0.5, op: 0.22 * k.lift }],
    };
  }
  if (px === 48) {
    return {
      shadow: [{ dy: 1.5, std: 1.5, op: 0.2 * k.shadow }],
      lift: [{ dy: 1, std: 1, op: 0.24 * k.lift }],
      rim: { w: 1, accent: 0.3, ink: 0.22 },
    };
  }
  return {
    shadow: [{ dy: 2, std: 1.8, op: 0.2 * k.shadow }],
    lift: [{ dy: 1.5, std: 1.5, op: 0.24 * k.lift }],
    rim: { w: 1, accent: 0.3, ink: 0.22 },
  };
}

export function macTuned(px) {
  const t = MAC_TUNED[px];
  return drawing({
    canvas: px,
    body: { x: t.bx, size: t.bs, shape: "mac" },
    plate: px <= 24 ? ["#F6F7FA", "#E4E7ED"] : ["#FBFBFD", "#E9ECF1"],
    edge:
      px >= 48 ? { w: 1, top: ["#FFFFFF", 0.9], bottom: ["#000000", 0.08] } : null,
    mark: { x: t.mx, y: t.mx, sq: t.sq, gap: t.gap, r: Math.round(t.sq * 0.27 * 4) / 4 },
    fills: px <= 24 ? { accent: ACCENT_FLAT, ink: INK_FLAT } : { accent: ACCENT, ink: INK },
    ...effectsFor(px, { shadow: 1, lift: 1 }),
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
    mark: { x: 245, y: 245, sq: 242, gap: 50, r: 65 },
    fills: { accent: ["#3777E6", "#2C68D6"], ink: ["#2A2E37", "#20232B"] },
    shadow: [{ dy: 8, std: 8, op: 0.14 }],
    lift: [{ dy: 6, std: 6, op: 0.2 }],
  });
}

// Windows tiles are full bleed: the plate is the whole canvas.
const WIN_TUNED = {
  16: { mx: 2, sq: 5, gap: 2 },
  20: { mx: 3, sq: 6, gap: 2 },
  24: { mx: 4, sq: 7, gap: 2 },
  32: { mx: 6, sq: 9, gap: 2 },
  40: { mx: 8, sq: 10, gap: 4 },
  48: { mx: 9, sq: 13, gap: 4 },
  64: { mx: 12, sq: 18, gap: 4 },
};

export function winTuned(px) {
  const t = WIN_TUNED[px];
  return drawing({
    canvas: px,
    body: { x: 0, size: px, shape: "win" },
    plate: ["#FFFFFF", "#EDEFF4"],
    edge: { w: px <= 24 ? 0.5 : 1, flat: "#000000", flatOp: px <= 24 ? 0.14 : 0.1 },
    mark: { x: t.mx, y: t.mx, sq: t.sq, gap: t.gap, r: Math.round(t.sq * 0.27 * 4) / 4 },
    fills: px <= 24 ? { accent: ACCENT_FLAT, ink: INK_FLAT } : { accent: ["#3777E6", "#2C68D6"], ink: ["#2A2E37", "#20232B"] },
    lift: px >= 40 ? [{ dy: px / 170, std: px / 170, op: 0.2 }] : [],
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
    mark: { x: 256, y: 254, sq: 228, gap: 48, r: 61 },
    fills: { accent: ACCENT, ink: INK },
    shadow: [{ dy: 12, std: 14, op: 0.2 }, { dy: 3, std: 2.5, op: 0.2 }],
    lift: [{ dy: 10, std: 10, op: 0.14 }, { dy: 3, std: 2.5, op: 0.26 }],
    rim: { w: 1.5, accent: 0.28, ink: 0.2 },
  });
}

const LINUX_TUNED = {
  16: { bx: 1, bs: 14, mx: 2, sq: 5, gap: 2 },
  22: { bx: 1, bs: 20, mx: 3, sq: 7, gap: 2 },
  24: { bx: 1, bs: 22, mx: 3, sq: 8, gap: 2 },
  32: { bx: 2, bs: 28, mx: 5, sq: 10, gap: 2 },
  48: { bx: 3, bs: 42, mx: 8, sq: 14, gap: 4 },
  64: { bx: 4, bs: 56, mx: 12, sq: 18, gap: 4 },
};

export function linuxTuned(px) {
  const t = LINUX_TUNED[px];
  return drawing({
    canvas: px,
    body: { x: t.bx, size: t.bs, shape: "linux" },
    plate: px <= 24 ? ["#F6F7FA", "#E4E7ED"] : ["#FAFAFC", "#E6E9EF"],
    edge: px <= 24 ? null : { w: 1, flat: "#000000", flatOp: 0.12 },
    mark: { x: t.mx, y: t.mx, sq: t.sq, gap: t.gap, r: Math.round(t.sq * 0.27 * 4) / 4 },
    fills: px <= 24 ? { accent: ACCENT_FLAT, ink: INK_FLAT } : { accent: ACCENT, ink: INK },
    ...effectsFor(px, { shadow: 1, lift: 1 }),
  });
}

export const TUNED_SIZES = {
  mac: Object.keys(MAC_TUNED).map(Number),
  win: Object.keys(WIN_TUNED).map(Number),
  linux: Object.keys(LINUX_TUNED).map(Number),
};
