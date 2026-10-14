// The review sheet: docs/design/icon-sheet.png. One SVG built from the rasters the generator just
// made, then rasterized with resvg. Every row is a grid: fixed column positions, items bottom-aligned
// to one baseline per row, each label 24 px under its baseline. Small sizes get two sub-rows, native
// pixels and then the same pixels magnified 4x with nearest-neighbour, in the same columns.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { magnifyPng } from "./icon_io.mjs";

const W = 2400;
const M = 48; // side margin
const GAP = 48; // gap between columns
const LABEL = 24; // baseline to label baseline
const FONT = `font-family="Helvetica Neue, Helvetica, Arial, sans-serif"`;
const THEME = {
  light: { bg: "#F5F5F7", fg: "#1D1D1F", mute: "#6E6E73" },
  dark: { bg: "#1C1C1E", fg: "#F5F5F7", mute: "#98989D" },
};

export function writeSheet({ iconPng, darkSvg, resvg, work, designDir }) {
  const darkPng = (px) => {
    const out = join(work, `dark-${px}.png`);
    execFileSync(resvg, ["-w", String(px), "-h", String(px), darkSvg, out]);
    return readFileSync(out);
  };
  let body = "";
  let y = 0;

  const text = (x, yy, s, fill, size = 14, weight = 400, anchor = "start") =>
    `<text x="${x}" y="${yy}" ${FONT} font-size="${size}" font-weight="${weight}" text-anchor="${anchor}" fill="${fill}">${s}</text>`;
  const img = (png, x, yy, w) =>
    `<image x="${x}" y="${yy}" width="${w}" height="${w}" href="data:image/png;base64,${png.toString("base64")}"/>`;

  /**
   * Items of one row on a shared baseline: [{png, px, x, label}]. Each item is bottom-aligned to
   * `base` (its top is base - px) and labelled directly under the baseline.
   */
  const row = (t, base, items) => {
    for (const it of items) {
      body += img(it.png, it.x, base - it.px, it.px);
      body += text(it.x, base + LABEL, it.label, t.mute, 13);
    }
  };
  /** Lays items out left to right with fixed gaps, returning them with x filled in. */
  const flow = (x0, items) => {
    let x = x0;
    return items.map((it) => {
      const out = { ...it, x };
      x += it.px + GAP;
      return out;
    });
  };
  /** A column set for small sizes: native sub-row, then the 4x sub-row, same x per column. */
  const smallRows = (t, x0, top, platform, sizes, slot) => {
    // Columns are as wide as the 4x preview of their size, so nothing overlaps.
    let x = x0;
    const native = [];
    const mag = [];
    for (const px of sizes) {
      const png = iconPng(platform, px);
      native.push({ png, px, x, label: `${px}` });
      mag.push({ png: magnifyPng(png, 4), px: px * 4, x, label: `${px} at 4x` });
      x += px * 4 + GAP;
    }
    const baseNative = top + 64; // the tallest native item is 64 px
    row(t, baseNative, native);
    const baseMag = baseNative + LABEL + 40 + sizes[0] * 4;
    row(t, baseMag, mag);
    return { end: x - GAP, bottom: baseMag + LABEL };
  };
  const runBand = (mode, title, note, build) => {
    const t = THEME[mode];
    const start = y;
    const before = body.length;
    body += text(M, start + 44, title, t.fg, 20, 600) + text(W - M, start + 44, note, t.mute, 14, 400, "end");
    const bottom = build(t, start);
    const height = bottom - start + 48;
    body = body.slice(0, before) + `<rect x="0" y="${start}" width="${W}" height="${height}" fill="${t.bg}"/>` + body.slice(before);
    y = start + height;
  };

  // ---- macOS: big sizes on one baseline, then the small sizes in two aligned sub-rows ----
  for (const mode of ["light", "dark"]) {
    runBand(mode, `macOS, ${mode} desktop`, "default icon; the dark plate variant sits at the right of the first row", (t, top) => {
      const base1 = top + 80 + 512;
      const big = flow(M, [
        { png: iconPng("macos", 288), px: 288, label: "1024 (scaled to 288)" },
        { png: iconPng("macos", 512), px: 512, label: "512" },
        { png: iconPng("macos", 256), px: 256, label: "256" },
        { png: iconPng("macos", 128), px: 128, label: "128" },
      ]);
      row(t, base1, big);
      const last = big[big.length - 1];
      const dx = last.x + last.px + GAP * 2;
      body += text(dx, top + 80 + 512 - 256 - 24, "Dark plate variant", t.fg, 14, 600);
      row(
        t,
        base1,
        flow(dx, [
          { png: darkPng(256), px: 256, label: "256" },
          { png: darkPng(128), px: 128, label: "128" },
          { png: darkPng(64), px: 64, label: "64" },
        ]),
      );
      const small = smallRows(t, M, base1 + LABEL + 72, "macos", [64, 48, 32, 24, 20, 16]);
      body += text(M, base1 + LABEL + 48, "Small sizes: each has its own pixel-grid drawing. Native, then 4x.", t.mute, 14);
      if (small.end > W - M) throw new Error(`macOS small row too wide: ${small.end}`);
      return small.bottom;
    });
  }

  // ---- Windows (Fluent) and Linux (GNOME): the same grid in two halves ----
  const HALF = W / 2;
  for (const mode of ["light", "dark"]) {
    runBand(mode, `Windows (Fluent) and Linux (GNOME), ${mode}`, "256 native; 64, 32 and 16 native, then 4x", (t, top) => {
      let bottom = top;
      for (const [platform, x0, name] of [["windows", M, "Windows"], ["linux", HALF + M, "Linux"]]) {
        body += text(x0, top + 84, name, t.fg, 14, 600);
        const base1 = top + 100 + 256;
        row(t, base1, [{ png: iconPng(platform, 256), px: 256, label: "256", x: x0 }]);
        const small = smallRows(t, x0, base1 + LABEL + 48, platform, [64, 32, 16]);
        if (small.end > x0 + HALF - 2 * M) throw new Error(`${platform} small row too wide`);
        bottom = Math.max(bottom, small.bottom);
      }
      return bottom;
    });
  }

  const H = y;
  const svgPath = join(work, "icon-sheet.svg");
  writeFileSync(svgPath, `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">${body}</svg>`);
  execFileSync(resvg, ["-w", String(W), svgPath, join(designDir, "icon-sheet.png")]);
}
