import { site } from "./site";

export interface ReleaseAsset { name: string; url: string; size: number }
export interface Release {
  tag: string
  htmlUrl: string
  publishedAt?: string
  assets: ReleaseAsset[]
  /** Resolved per-platform downloads (undefined when the asset is missing). */
  downloads: {
    macArm?: ReleaseAsset; macX64?: ReleaseAsset
    winExe?: ReleaseAsset; winMsi?: ReleaseAsset
    deb?: ReleaseAsset; rpm?: ReleaseAsset; appImage?: ReleaseAsset
  }
}

const FALLBACK: Release = { tag: "latest", htmlUrl: site.latest, assets: [], downloads: {} };

function pick(assets: ReleaseAsset[], re: RegExp): ReleaseAsset | undefined {
  return assets.find((a) => re.test(a.name) && !a.name.endsWith(".sig"));
}

let cached: Promise<Release> | null = null

/** Latest GitHub release, resolved once per build. Falls back to the releases page when the API is unavailable. */
export function getLatestRelease(): Promise<Release> {
  if (!cached) cached = fetchRelease();
  return cached;
}

async function fetchRelease(): Promise<Release> {
  try {
    const headers: Record<string, string> = { Accept: "application/vnd.github+json", "User-Agent": "caret-website" };
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    const res = await fetch("https://api.github.com/repos/Fast-Editor/Caret/releases/latest", { headers });
    if (!res.ok) return FALLBACK;
    const json = (await res.json()) as { tag_name: string; html_url: string; published_at?: string; assets: { name: string; browser_download_url: string; size: number }[] };
    const assets: ReleaseAsset[] = json.assets.map((a) => ({ name: a.name, url: a.browser_download_url, size: a.size }));
    return {
      tag: json.tag_name, htmlUrl: json.html_url, publishedAt: json.published_at, assets,
      downloads: {
        macArm: pick(assets, /aarch64\.dmg$/), macX64: pick(assets, /x64\.dmg$/),
        winExe: pick(assets, /-setup\.exe$/), winMsi: pick(assets, /\.msi$/),
        deb: pick(assets, /\.deb$/), rpm: pick(assets, /\.rpm$/), appImage: pick(assets, /\.AppImage$/),
      },
    };
  } catch {
    return FALLBACK;
  }
}

export function formatSize(bytes: number): string {
  if (!bytes) return "";
  const mb = bytes / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(0)} MB` : `${(bytes / 1024).toFixed(0)} KB`;
}
