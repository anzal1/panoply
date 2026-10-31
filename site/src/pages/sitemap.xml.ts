import { tools } from "../data/tools";
import presets from "../data/exam-presets.json";
import rules from "../data/passport-rules.json";
export function GET() {
  const base = "https://panoply.anzalabidi.dev";
  const urls = ["/", "/tools/", ...tools.map((t) => `/tools/${t.slug}/`), ...presets.map((p) => `/tools/resize-to-kb/${p.id}/`), ...rules.map((r) => `/tools/passport-photo/${r.id}/`)];
  const body = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.map((u) => `  <url><loc>${base}${u}</loc></url>`).join("\n")}\n</urlset>\n`;
  return new Response(body, { headers: { "Content-Type": "application/xml" } });
}
