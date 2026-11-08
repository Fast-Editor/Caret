import type { Metadata } from "next";
import { pageMeta } from "@/lib/seo";
import { readRepoFile } from "@/lib/docs";
import { renderMarkdown } from "@/lib/markdown";

export const metadata: Metadata = pageMeta({ title: "Changelog", description: "What changed in each Caret release: new features, fixes and settings, from the repository changelog.", path: "/changelog" });

export default async function ChangelogPage() {
  const md = await readRepoFile("CHANGELOG.md");
  const { html } = await renderMarkdown(md, { base: "root", stripTitle: true });
  return (
    <div className="mx-auto max-w-3xl px-4 py-10 sm:px-6">
      <h1 className="text-3xl font-semibold tracking-tight">Changelog</h1>
      <article className="prose mt-6" dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}
