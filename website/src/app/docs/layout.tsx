import Link from "next/link";
import { ScrollArea } from "@/components/ui/scroll-area";
import { listDocs } from "@/lib/docs";
import { DocsNav } from "./nav";

export default async function DocsLayout({ children }: LayoutProps<"/docs">) {
  const docs = await listDocs();
  return (
    <div className="mx-auto flex max-w-6xl gap-10 px-4 py-10 sm:px-6">
      <aside className="hidden w-56 shrink-0 lg:block">
        <ScrollArea className="sticky top-20 h-[calc(100vh-6rem)] pr-3">
          <div className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">Documentation</div>
          <DocsNav items={[{ slug: "", title: "Overview" }, ...docs.map((d) => ({ slug: d.slug, title: d.title }))]} />
        </ScrollArea>
      </aside>
      <div className="min-w-0 flex-1">
        <div className="mb-6 lg:hidden">
          <details className="rounded-lg border border-border bg-muted/30 px-4 py-2 text-sm">
            <summary className="cursor-pointer font-medium">All documentation</summary>
            <ul className="mt-2 flex flex-col gap-1 pb-2">
              <li><Link className="text-muted-foreground hover:text-foreground" href="/docs">Overview</Link></li>
              {docs.map((d) => <li key={d.slug}><Link className="text-muted-foreground hover:text-foreground" href={`/docs/${d.slug}`}>{d.title}</Link></li>)}
            </ul>
          </details>
        </div>
        {children}
      </div>
    </div>
  );
}
