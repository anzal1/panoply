#!/usr/bin/env node
// Generates the Panoply app icon: SVG sources, then every raster the three desktops need, then the
// review sheet. One command regenerates all of it:
//
//   node apps/desktop/tools/gen_app_icon.mjs [--resvg PATH] [--no-sheet]
//
// The mark is the Fan (see icon_art.mjs): three tall rounded cards fanned from one pivot, the front
// card in the accent blue (#2D6CDF) with a plate-coloured stroke around it and a rivet at the pivot.
// The drawings live in icon_art.mjs, one per platform:
//
//   macOS    panoply-macos.svg (1024 canvas, 824 squircle body, baked shadow), panoply-macos-dark.svg
//            (a dark plate variant, reference only), tuned panoply-macos-{16,20,24,32,48,64}.svg
//   Windows  panoply-windows.svg (Fluent tile), tuned panoply-windows-{16,20,24,32,40,48,64}.svg
//   Linux    panoply-linux.svg (GNOME style, shadow baked in), tuned panoply-linux-{16,22,24,32,48,64}.svg
//
// It also writes the flat shared geometry for the app UI (packaging/icon/fan-layers/*.svg) and the
// README brand assets (docs/assets/panoply-mark*.svg, and the fan at the start of panoply-logo-*.svg,
// keeping the wordmark paths as they are).
//
// Below 128 px nothing is a scaled copy: each tuned drawing is laid out on the pixel grid so square
// edges land on whole pixels. Needs `resvg` (MIT, `cargo install resvg`) on PATH or via --resvg.
// Everything else (.icns, .ico, PNG) is written here, so the script runs on any OS. The generated
// files are committed; run this only when the drawing changes.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import * as art from "./icon_art.mjs";
import { buildIcns, buildIco } from "./icon_io.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(here, "../packaging");
const iconDir = join(pkg, "icon");
const designDir = resolve(here, "../../../docs/design");
const assetsDir = resolve(here, "../../../docs/assets");
const argv = process.argv.slice(2);
const resvg = argv.includes("--resvg") ? argv[argv.indexOf("--resvg") + 1] : "resvg";

// ---- SVG sources ----
const platforms = {
  macos: { master: () => art.macMaster(), tuned: art.macTuned, sizes: art.TUNED_SIZES.mac },
  windows: { master: () => art.winMaster(), tuned: art.winTuned, sizes: art.TUNED_SIZES.win },
  linux: { master: () => art.linuxMaster(), tuned: art.linuxTuned, sizes: art.TUNED_SIZES.linux },
};
mkdirSync(iconDir, { recursive: true });
for (const old of ["panoply.svg", "panoply-small.svg", "panoply-macos-small.svg"]) {
  rmSync(join(iconDir, old), { force: true });
}
for (const [name, p] of Object.entries(platforms)) {
  writeFileSync(join(iconDir, `panoply-${name}.svg`), p.master());
  for (const px of p.sizes) writeFileSync(join(iconDir, `panoply-${name}-${px}.svg`), p.tuned(px));
}
writeFileSync(join(iconDir, "panoply-macos-dark.svg"), art.macMaster(true));

// ---- Shared flat geometry for the app UI ----
mkdirSync(join(iconDir, "fan-layers"), { recursive: true });
for (const [name, svg] of Object.entries(art.fanLayers())) writeFileSync(join(iconDir, "fan-layers", name), svg);

// ---- README brand assets ----
writeFileSync(join(assetsDir, "panoply-mark.svg"), art.markSvg(false));
writeFileSync(join(assetsDir, "panoply-mark-dark.svg"), art.markSvg(true));
for (const [file, dark] of [["panoply-logo-light.svg", false], ["panoply-logo-dark.svg", true]]) {
  // Replace everything between the title and the first wordmark path (the one starting M713.); the
  // wordmark paths stay byte for byte as they are, so this is safe to run repeatedly.
  const p = join(assetsDir, file);
  const svg = readFileSync(p, "utf8");
  writeFileSync(p, svg.replace(/(<title>Panoply<\/title>).*?(?=<path d="M713\.)/s, `$1${art.logoFan(dark)}`));
}

const work = join(tmpdir(), `panoply-icon-${process.pid}`);
mkdirSync(work, { recursive: true });
const cache = new Map();

/** PNG bytes of a platform icon at `px`: the tuned drawing when one exists, else the master scaled. */
export function iconPng(platform, px) {
  const key = `${platform}-${px}`;
  if (cache.has(key)) return cache.get(key);
  const tuned = platforms[platform].sizes.includes(px);
  const svg = join(iconDir, tuned ? `panoply-${platform}-${px}.svg` : `panoply-${platform}.svg`);
  const out = join(work, `${key}.png`);
  execFileSync(resvg, ["-w", String(px), "-h", String(px), svg, out]);
  const data = readFileSync(out);
  cache.set(key, data);
  return data;
}

// ---- macOS .icns: the @1x and @2x pair for 16, 32, 128, 256 and 512 ----
const icnsTypes = [
  ["icp4", 16], // 16x16
  ["icp5", 32], // 32x32
  ["ic07", 128], // 128x128
  ["ic08", 256], // 256x256
  ["ic09", 512], // 512x512
  ["ic10", 1024], // 512x512@2x
  ["ic11", 32], // 16x16@2x
  ["ic12", 64], // 32x32@2x
  ["ic13", 256], // 128x128@2x
  ["ic14", 512], // 256x256@2x
];
mkdirSync(join(pkg, "macos"), { recursive: true });
writeFileSync(
  join(pkg, "macos/Panoply.icns"),
  buildIcns(icnsTypes.map(([type, px]) => [type, iconPng("macos", px)])),
);

// ---- Windows .ico: 32-bit BMP entries below 256 px, a PNG entry at 256 ----
const icoSizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
mkdirSync(join(pkg, "windows"), { recursive: true });
writeFileSync(join(pkg, "windows/panoply.ico"), buildIco(icoSizes.map((px) => [px, iconPng("windows", px)])));

// ---- Linux hicolor theme ----
for (const px of [16, 22, 24, 32, 48, 64, 128, 256, 512]) {
  const d = join(pkg, `linux/icons/hicolor/${px}x${px}/apps`);
  mkdirSync(d, { recursive: true });
  writeFileSync(join(d, "panoply.png"), iconPng("linux", px));
}
const sc = join(pkg, "linux/icons/hicolor/scalable/apps");
mkdirSync(sc, { recursive: true });
writeFileSync(join(sc, "panoply.svg"), readFileSync(join(iconDir, "panoply-linux.svg")));

// ---- Review material in docs/design ----
mkdirSync(designDir, { recursive: true });
const darkOut = join(designDir, "icon-dark-variant.png");
execFileSync(resvg, ["-w", "1024", "-h", "1024", join(iconDir, "panoply-macos-dark.svg"), darkOut]);
if (!argv.includes("--no-sheet")) {
  const { writeSheet } = await import("./icon_sheet.mjs");
  writeSheet({ iconPng, darkSvg: join(iconDir, "panoply-macos-dark.svg"), resvg, work, designDir });
}

rmSync(work, { recursive: true, force: true });
console.log("Wrote packaging/icon SVGs, macos/Panoply.icns, windows/panoply.ico, linux/icons/hicolor and docs/design.");
