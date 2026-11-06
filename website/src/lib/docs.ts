import { promises as fs } from "node:fs";
import path from "node:path";
import { docSlug } from "./markdown";

/** The repository's docs folder: the website is built from the same checkout. */
export const DOCS_DIR = path.resolve(process.cwd(), "..", "docs");
export const REPO_ROOT = path.resolve(process.cwd(), "..");

export interface DocMeta { slug: string; file: string; title: string; summary: string }

/** Order and one-line summaries come from the table in docs/README.md; anything not listed there is appended alphabetically. */
export async function listDocs(): Promise<DocMeta[]> {
  const files = (await fs.readdir(DOCS_DIR)).filter((f) => f.endsWith(".md") && f !== "README.md");
  const index = await fs.readFile(path.join(DOCS_DIR, "README.md"), "utf8").catch(() => "");
  const ordered: DocMeta[] = [];
  const seen = new Set<string>();
  for (const line of index.split("\n")) {
    const m = /^\|\s*\[([^\]]+\.md)\]\([^)]+\)\s*\|\s*(.+?)\s*\|\s*$/.exec(line);
    if (!m || !files.includes(m[1])) continue;
    seen.add(m[1]);
    ordered.push({ slug: docSlug(m[1]), file: m[1], title: await titleOf(m[1]), summary: m[2].replace(/`/g, "") });
  }
  for (const f of files.sort()) if (!seen.has(f)) ordered.push({ slug: docSlug(f), file: f, title: await titleOf(f), summary: "" });
  return ordered;
}

async function titleOf(file: string): Promise<string> {
  const text = await fs.readFile(path.join(DOCS_DIR, file), "utf8");
  const m = /^#\s+(.+)$/m.exec(text);
  return m ? m[1].trim().replace(/`/g, "") : file.replace(/\.md$/, "");
}

export async function readDoc(slug: string): Promise<{ meta: DocMeta; markdown: string } | null> {
  const docs = await listDocs();
  const meta = docs.find((d) => d.slug === slug);
  if (!meta) return null;
  return { meta, markdown: await fs.readFile(path.join(DOCS_DIR, meta.file), "utf8") };
}

export async function readRepoFile(rel: string): Promise<string> {
  return fs.readFile(path.join(REPO_ROOT, rel), "utf8");
}
