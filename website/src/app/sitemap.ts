import type { MetadataRoute } from "next";
import { promises as fs } from "node:fs";
import path from "node:path";
import { DOCS_DIR, REPO_ROOT, listDocs } from "@/lib/docs";
import { site } from "@/lib/site";

export const dynamic = "force-static";

async function mtime(file: string): Promise<Date> {
  try { return (await fs.stat(file)).mtime; } catch { return new Date(); }
}

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  const docs = await listDocs();
  const now = new Date();
  const entries: MetadataRoute.Sitemap = [
    { url: `${site.url}/`, lastModified: now, changeFrequency: "weekly", priority: 1 },
    { url: `${site.url}/download`, lastModified: now, changeFrequency: "weekly", priority: 0.9 },
    { url: `${site.url}/compare`, lastModified: await mtime(path.join(REPO_ROOT, "website/src/lib/site.ts")), changeFrequency: "monthly", priority: 0.8 },
    { url: `${site.url}/changelog`, lastModified: await mtime(path.join(REPO_ROOT, "CHANGELOG.md")), changeFrequency: "weekly", priority: 0.6 },
    { url: `${site.url}/docs`, lastModified: await mtime(path.join(DOCS_DIR, "README.md")), changeFrequency: "weekly", priority: 0.7 },
  ];
  for (const d of docs) entries.push({ url: `${site.url}/docs/${d.slug}`, lastModified: await mtime(path.join(DOCS_DIR, d.file)), changeFrequency: "monthly", priority: 0.6 });
  return entries;
}
