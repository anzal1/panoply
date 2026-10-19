// The newest release (prereleases included), read at build time so every link, size and checksum on the page is current.
// In CI a failed read fails the build; offline it falls back to the committed snapshot.
import fallback from "./release.fallback.json";

export type Asset = { name: string; size: number; url: string; sha?: string };
export type Release = { tag: string; version: string; url: string; published: string; assets: Asset[] };

const REPO = "anzal1/panoply";

export async function latestRelease(): Promise<Release> {
  try {
    const headers: Record<string, string> = { accept: "application/vnd.github+json", "user-agent": "panoply-site" };
    if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=5`, { headers });
    if (!res.ok) throw new Error(`GitHub releases answered ${res.status}`);
    const rel = (await res.json()).find((r: any) => !r.draft);
    const sumsAsset = rel.assets.find((a: any) => a.name === "SHA256SUMS");
    const sha: Record<string, string> = {};
    if (sumsAsset) {
      const text = await (await fetch(sumsAsset.browser_download_url, { headers: { "user-agent": "panoply-site" } })).text();
      for (const line of text.split("\n")) {
        const [hash, name] = line.trim().split(/\s+/);
        if (hash && name) sha[name.replace(/^\*/, "")] = hash;
      }
    }
    return {
      tag: rel.tag_name,
      version: rel.tag_name.replace(/^v/, ""),
      url: rel.html_url,
      published: rel.published_at,
      assets: rel.assets.map((a: any) => ({ name: a.name, size: a.size, url: a.browser_download_url, sha: sha[a.name] })),
    };
  } catch (e) {
    if (process.env.CI) throw e;
    console.warn(`release: using the offline snapshot (${(e as Error).message})`);
    return fallback as Release;
  }
}

export const mb = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`;
