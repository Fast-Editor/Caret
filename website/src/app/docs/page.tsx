import type { Metadata } from "next";
import { pageMeta } from "@/lib/seo";
import Link from "next/link";
import { ArrowRight } from "lucide-react";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { listDocs, readRepoFile } from "@/lib/docs";
import { renderMarkdown } from "@/lib/markdown";

export const metadata: Metadata = pageMeta({ title: "Documentation", description: "Caret reference documentation: architecture, providers, auto routing, external agents, sandbox, terminal, debugging, CLI and releasing.", path: "/docs" });

export default async function DocsIndex() {
  const docs = await listDocs();
  const md = await readRepoFile("docs/README.md");
  // The index table is rendered as cards below; keep the prose around it.
  const intro = md.replace(/^\|.*\|\s*$/gm, "").replace(/\n{3,}/g, "\n\n").replace(/^## Guides\s*$/m, "");
  const { html } = await renderMarkdown(intro, { base: "docs", stripTitle: true });
  return (
    <article>
      <h1 className="text-3xl font-semibold tracking-tight">Documentation</h1>
      <div className="prose mt-4" dangerouslySetInnerHTML={{ __html: html }} />
      <h2 className="mb-4 mt-10 text-xl font-semibold tracking-tight">Guides</h2>
      <div className="grid gap-3 sm:grid-cols-2">
        {docs.map((d) => (
          <Link key={d.slug} href={`/docs/${d.slug}`} className="group">
            <Card className="h-full gap-2 py-4 transition-colors group-hover:border-foreground/30">
              <CardHeader className="px-4">
                <CardTitle className="flex items-center justify-between text-base">{d.title}<ArrowRight className="size-4 text-muted-foreground transition-transform group-hover:translate-x-0.5" /></CardTitle>
                {d.summary && <CardDescription>{d.summary}</CardDescription>}
              </CardHeader>
            </Card>
          </Link>
        ))}
      </div>
    </article>
  );
}
