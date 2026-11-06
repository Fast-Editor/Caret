import { unified, type Plugin } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkRehype from "remark-rehype";
import rehypeSlug from "rehype-slug";
import rehypeAutolinkHeadings from "rehype-autolink-headings";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeStringify from "rehype-stringify";
import { visit } from "unist-util-visit";
import { toString as hastToString } from "hast-util-to-string";
import type { Root, Element } from "hast";
import { site } from "./site";

export interface TocEntry { depth: 2 | 3; id: string; text: string }
export interface Rendered { html: string; toc: TocEntry[]; title: string | null }

/** `ARCHITECTURE.md` → `architecture`, `UI_COMMANDS.md` → `ui-commands`. */
export function docSlug(fileName: string): string {
  return fileName.replace(/\.md$/i, "").toLowerCase().replace(/_/g, "-");
}

/** Rewrite the relative links docs use on GitHub into site routes (or GitHub blob URLs for source files). */
const rehypeRewriteLinks: Plugin<[{ base: "docs" | "root" }], Root> = ({ base }) => (tree) => {
  visit(tree, "element", (node: Element) => {
    if (node.tagName !== "a" || typeof node.properties?.href !== "string") return;
    const href = node.properties.href as string;
    if (/^(https?:|mailto:|#)/.test(href)) { if (/^https?:/.test(href)) { node.properties.target = "_blank"; node.properties.rel = ["noopener", "noreferrer"]; } return; }
    const [path, hash = ""] = href.split("#");
    const frag = hash ? `#${hash}` : "";
    const clean = path.replace(/^\.\//, "");
    // another doc
    let m = /^(?:docs\/)?([A-Za-z0-9_-]+)\.md$/.exec(clean);
    if (m && (base === "docs" || clean.startsWith("docs/"))) { node.properties.href = `/docs/${docSlug(m[1])}${frag}`; return; }
    if (clean === "" && hash) { node.properties.href = frag; return; }
    if (/^(\.\.\/)?README\.md$/.test(clean)) { node.properties.href = base === "docs" ? "/" : `/docs${frag}`; return; }
    if (/^(\.\.\/)?CHANGELOG\.md$/.test(clean)) { node.properties.href = `/changelog${frag}`; return; }
    if (/^(\.\.\/)?LICENSE$/.test(clean)) { node.properties.href = `${site.repo}/blob/main/LICENSE`; return; }
    m = /^(\.\.\/)?docs\/([A-Za-z0-9_-]+)\.md$/.exec(clean);
    if (m) { node.properties.href = `/docs/${docSlug(m[2])}${frag}`; return; }
    // anything else is a repository path relative to the markdown file's folder
    const rel = base === "docs" ? `docs/${clean}` : clean;
    const parts: string[] = [];
    for (const seg of rel.split("/")) { if (!seg || seg === ".") continue; if (seg === "..") { parts.pop(); continue; } parts.push(seg); }
    node.properties.href = `${site.repo}/blob/main/${parts.join("/")}${frag}`;
    node.properties.target = "_blank"; node.properties.rel = ["noopener", "noreferrer"];
  });
};

/** Collect h2/h3 (after rehype-slug assigned ids) and the first h1. */
const rehypeCollect: Plugin<[{ out: { toc: TocEntry[]; title: string | null } }], Root> = ({ out }) => (tree) => {
  visit(tree, "element", (node: Element) => {
    if (node.tagName === "h1" && out.title === null) out.title = hastToString(node);
    if ((node.tagName === "h2" || node.tagName === "h3") && typeof node.properties?.id === "string") {
      out.toc.push({ depth: node.tagName === "h2" ? 2 : 3, id: node.properties.id as string, text: hastToString(node) });
    }
  });
};

export async function renderMarkdown(markdown: string, opts: { base: "docs" | "root"; stripTitle?: boolean } = { base: "docs" }): Promise<Rendered> {
  const out = { toc: [] as TocEntry[], title: null as string | null };
  const file = await unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeSlug)
    .use(rehypeCollect, { out })
    .use(rehypeAutolinkHeadings, { behavior: "wrap", properties: { className: ["heading-link"] } })
    .use(rehypePrettyCode, { theme: { dark: "github-dark-default", light: "github-light" }, keepBackground: false, defaultLang: "text" })
    .use(rehypeRewriteLinks, { base: opts.base })
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process(markdown);
  let html = String(file);
  if (opts.stripTitle) html = html.replace(/^\s*<h1[\s\S]*?<\/h1>/, "");
  return { html, toc: out.toc, title: out.title };
}
