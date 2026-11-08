import type { Metadata } from "next";
import { promises as fs } from "node:fs";
import path from "node:path";
import { pageMeta } from "@/lib/seo";
import { JsonLd } from "@/components/site/json-ld";
import Link from "next/link";
import { notFound } from "next/navigation";
import { ExternalLink } from "lucide-react";
import { DOCS_DIR, listDocs, readDoc } from "@/lib/docs";
import { renderMarkdown } from "@/lib/markdown";
import { site } from "@/lib/site";

async function docModified(file: string): Promise<Date> {
  try { return (await fs.stat(path.join(DOCS_DIR, file))).mtime; } catch { return new Date(); }
}

export async function generateStaticParams() {
  return (await listDocs()).map((d) => ({ slug: d.slug }));
}

export async function generateMetadata({ params }: PageProps<"/docs/[slug]">): Promise<Metadata> {
  const { slug } = await params;
  const doc = await readDoc(slug);
  if (!doc) return {};
  return pageMeta({ title: doc.meta.title, description: doc.meta.summary || `${doc.meta.title}: Caret documentation.`, path: `/docs/${doc.meta.slug}`, type: "article", modified: await docModified(doc.meta.file) });
}

export default async function DocPage({ params }: PageProps<"/docs/[slug]">) {
  const { slug } = await params;
  const doc = await readDoc(slug);
  if (!doc) notFound();
  const modified = await docModified(doc.meta.file);
  const docs = await listDocs();
  const i = docs.findIndex((d) => d.slug === slug);
  const prev = i > 0 ? docs[i - 1] : null;
  const next = i >= 0 && i < docs.length - 1 ? docs[i + 1] : null;
  const { html, toc } = await renderMarkdown(doc.markdown, { base: "docs" });
  return (
    <div className="flex gap-10">
      <JsonLd data={{
        "@context": "https://schema.org",
        "@graph": [
          { "@type": "TechArticle", headline: doc.meta.title, description: doc.meta.summary || undefined, url: `${site.url}/docs/${doc.meta.slug}`, dateModified: modified.toISOString(), inLanguage: "en", author: { "@type": "Organization", name: site.name, url: site.url }, publisher: { "@type": "Organization", name: site.name, url: site.url, logo: { "@type": "ImageObject", url: `${site.url}/icon.png` } }, isPartOf: { "@type": "WebSite", name: site.name, url: site.url }, about: { "@type": "SoftwareApplication", name: site.name, url: site.url } },
          { "@type": "BreadcrumbList", itemListElement: [
            { "@type": "ListItem", position: 1, name: "Home", item: `${site.url}/` },
            { "@type": "ListItem", position: 2, name: "Docs", item: `${site.url}/docs` },
            { "@type": "ListItem", position: 3, name: doc.meta.title, item: `${site.url}/docs/${doc.meta.slug}` },
          ] },
        ],
      }} />
      <article className="min-w-0 flex-1">
        <div className="prose" dangerouslySetInnerHTML={{ __html: html }} />
        <div className="mt-10 flex items-center justify-between border-t border-border/60 pt-6 text-sm">
          {prev ? <Link href={`/docs/${prev.slug}`} className="text-muted-foreground hover:text-foreground">← {prev.title}</Link> : <span />}
          {next ? <Link href={`/docs/${next.slug}`} className="text-muted-foreground hover:text-foreground">{next.title} →</Link> : <span />}
        </div>
        <p className="mt-6 text-xs text-muted-foreground">
          <a href={`${site.repo}/blob/main/docs/${doc.meta.file}`} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 hover:text-foreground">Edit this page on GitHub <ExternalLink className="size-3" /></a>
        </p>
      </article>
      {toc.length > 2 && (
        <aside className="hidden w-52 shrink-0 xl:block">
          <div className="sticky top-20">
            <div className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">On this page</div>
            <ul className="flex flex-col gap-1 text-sm">
              {toc.map((t) => (
                <li key={t.id} className={t.depth === 3 ? "pl-3" : ""}>
                  <a href={`#${t.id}`} className="text-muted-foreground hover:text-foreground">{t.text}</a>
                </li>
              ))}
            </ul>
          </div>
        </aside>
      )}
    </div>
  );
}
