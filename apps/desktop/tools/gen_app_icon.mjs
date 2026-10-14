#!/usr/bin/env node
// Generates the Panoply app icon: SVG sources, then every raster the three desktops need.
//
//   apps/desktop/tools/gen_app_icon.mjs [--resvg PATH]
//
// The mark is the four-square Panoply mark from the wireframes: one accent square (#2D6CDF) and
// three ink squares, rounded, centred. Three drawings share it:
//   panoply-macos.svg   padded to macOS's icon grid (824 of 1024), soft shadow baked in
//   panoply.svg         full bleed, for Linux (hicolor scalable) and Windows
//   panoply-small.svg   the same mark with bigger squares, for 16 to 32 px where detail is lost
//
// Needs `resvg` (MIT, `cargo install resvg`) on PATH or via --resvg, and for the macOS .icns
// check only, `iconutil`. The .icns, .ico and PNG files are written here directly, so the script
// runs on any OS. The generated files are committed; run this only when the drawing changes.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(here, "../packaging");
const iconDir = join(pkg, "icon");
const argv = process.argv.slice(2);
const resvg = argv.includes("--resvg") ? argv[argv.indexOf("--resvg") + 1] : "resvg";

const BLUE = "#2D6CDF";

/** A superellipse (Apple's icon shape is close to exponent 5) as a closed SVG path. */
function squircle(x, y, size, n = 5, steps = 360) {
  const r = size / 2;
  const cx = x + r;
  const cy = y + r;
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t);
    const s = Math.sin(t);
    const px = cx + r * Math.sign(c) * Math.abs(c) ** (2 / n);
    const py = cy + r * Math.sign(s) * Math.abs(s) ** (2 / n);
    pts.push(`${px.toFixed(2)} ${py.toFixed(2)}`);
  }
  return `M${pts.join("L")}Z`;
}

/**
 * One drawing on a 1024 canvas.
 * `plate` is the side of the icon body, `sq` the side of each square and `gap` the space
 * between them. The four squares are centred, so the mark is balanced on the body.
 */
function drawing({ plate, sq, gap, radius, shadow, detail }) {
  const off = (1024 - plate) / 2;
  const grid = sq * 2 + gap;
  const g0 = (1024 - grid) / 2;
  const at = (col, row) => [g0 + col * (sq + gap), g0 + row * (sq + gap)];
  const square = (col, row, fill) => {
    const [x, y] = at(col, row);
    return `<rect x="${x}" y="${y}" width="${sq}" height="${sq}" rx="${radius}" fill="${fill}"${
      detail ? ' filter="url(#lift)"' : ""
    }/>${
      detail
        ? `<rect x="${x + 0.75}" y="${y + 0.75}" width="${sq - 1.5}" height="${sq - 1.5}" rx="${radius - 0.75}" fill="none" stroke="url(#rim)" stroke-width="1.5"/>`
        : ""
    }`;
  };
  const body = squircle(off, off, plate);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <title>Panoply</title>
  <defs>
    <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF"/>
      <stop offset="1" stop-color="#E7EAF1"/>
    </linearGradient>
    <linearGradient id="ink" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3B404C"/>
      <stop offset="1" stop-color="#1B1E26"/>
    </linearGradient>
    <linearGradient id="accent" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3F7DEB"/>
      <stop offset="0.55" stop-color="${BLUE}"/>
      <stop offset="1" stop-color="#2760CE"/>
    </linearGradient>
    <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/>
      <stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0"/>
    </linearGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.9"/>
      <stop offset="0.5" stop-color="#000000" stop-opacity="0.05"/>
      <stop offset="1" stop-color="#000000" stop-opacity="0.12"/>
    </linearGradient>
    <filter id="drop" x="-20%" y="-20%" width="140%" height="150%">
      <feDropShadow dx="0" dy="${shadow ? 12 : 0}" stdDeviation="${shadow ? 16 : 0}" flood-color="#0B1020" flood-opacity="0.30"/>
    </filter>
    <filter id="lift" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="0" dy="${sq * 0.03}" stdDeviation="${sq * 0.04}" flood-color="#0B1020" flood-opacity="0.22"/>
    </filter>
  </defs>
  <path d="${body}" fill="url(#plate)"${shadow ? ' filter="url(#drop)"' : ""}/>
  ${detail ? `<path d="${body}" fill="none" stroke="url(#edge)" stroke-width="2"/>` : ""}
  ${square(0, 0, "url(#accent)")}
  ${square(1, 0, "url(#ink)")}
  ${square(0, 1, "url(#ink)")}
  ${square(1, 1, "url(#ink)")}
</svg>
`;
}

// macOS grid: the body is 824 of 1024 (80.5 percent), the mark about 54 percent of the body.
const variants = {
  "panoply-macos.svg": drawing({ plate: 824, sq: 212, gap: 42, radius: 58, shadow: true, detail: true }),
  "panoply.svg": drawing({ plate: 1024, sq: 246, gap: 52, radius: 66, shadow: false, detail: true }),
  "panoply-small.svg": drawing({ plate: 1024, sq: 284, gap: 40, radius: 62, shadow: false, detail: false }),
  // The macOS drawing at small sizes: less padding and bigger squares so 16 px still reads.
  "panoply-macos-small.svg": drawing({ plate: 940, sq: 262, gap: 36, radius: 58, shadow: false, detail: false }),
};
mkdirSync(iconDir, { recursive: true });
for (const [name, svg] of Object.entries(variants)) writeFileSync(join(iconDir, name), svg);

const work = join(tmpdir(), `panoply-icon-${process.pid}`);
mkdirSync(work, { recursive: true });
function png(svgName, px) {
  const out = join(work, `${svgName}-${px}.png`);
  execFileSync(resvg, ["-w", String(px), "-h", String(px), join(iconDir, svgName), out]);
  return readFileSync(out);
}

// ---- macOS .icns: PNG payloads under the modern type codes ----
const icnsTypes = [
  ["icp4", 16], ["icp5", 32], ["icp6", 64], ["ic07", 128], ["ic08", 256], ["ic09", 512], ["ic10", 1024],
  ["ic11", 32], ["ic12", 64], ["ic13", 256], ["ic14", 512],
];
const macSource = (px) => (px <= 64 ? "panoply-macos-small.svg" : "panoply-macos.svg");
const chunks = icnsTypes.map(([type, px]) => {
  const data = png(macSource(px), px);
  const head = Buffer.alloc(8);
  head.write(type, 0, "ascii");
  head.writeUInt32BE(8 + data.length, 4);
  return Buffer.concat([head, data]);
});
const icnsBody = Buffer.concat(chunks);
const icnsHead = Buffer.alloc(8);
icnsHead.write("icns", 0, "ascii");
icnsHead.writeUInt32BE(8 + icnsBody.length, 4);
mkdirSync(join(pkg, "macos"), { recursive: true });
writeFileSync(join(pkg, "macos/Panoply.icns"), Buffer.concat([icnsHead, icnsBody]));

// ---- Windows .ico: PNG-compressed entries ----
const small = (px) => (px <= 32 ? "panoply-small.svg" : "panoply.svg");
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const icoImages = icoSizes.map((px) => png(small(px), px));
const dir = Buffer.alloc(6 + 16 * icoSizes.length);
dir.writeUInt16LE(0, 0);
dir.writeUInt16LE(1, 2);
dir.writeUInt16LE(icoSizes.length, 4);
let offset = dir.length;
icoSizes.forEach((px, i) => {
  const e = 6 + 16 * i;
  dir.writeUInt8(px >= 256 ? 0 : px, e);
  dir.writeUInt8(px >= 256 ? 0 : px, e + 1);
  dir.writeUInt16LE(1, e + 4);
  dir.writeUInt16LE(32, e + 6);
  dir.writeUInt32LE(icoImages[i].length, e + 8);
  dir.writeUInt32LE(offset, e + 12);
  offset += icoImages[i].length;
});
mkdirSync(join(pkg, "windows"), { recursive: true });
writeFileSync(join(pkg, "windows/panoply.ico"), Buffer.concat([dir, ...icoImages]));

// ---- Linux hicolor theme ----
for (const px of [16, 22, 24, 32, 48, 64, 128, 256, 512]) {
  const d = join(pkg, `linux/icons/hicolor/${px}x${px}/apps`);
  mkdirSync(d, { recursive: true });
  writeFileSync(join(d, "panoply.png"), png(small(px), px));
}
const sc = join(pkg, "linux/icons/hicolor/scalable/apps");
mkdirSync(sc, { recursive: true });
writeFileSync(join(sc, "panoply.svg"), readFileSync(join(iconDir, "panoply.svg")));

rmSync(work, { recursive: true, force: true });
console.log("Wrote the SVGs in packaging/icon, macos/Panoply.icns, windows/panoply.ico and linux/icons/hicolor.");
