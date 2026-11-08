import Link from "next/link";
import { site } from "@/lib/site";

export function Footer() {
  return (
    <footer className="border-t border-border/60">
      <div className="mx-auto flex max-w-6xl flex-col gap-6 px-4 py-10 text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between sm:px-6">
        <div>
          <div className="font-medium text-foreground">Caret</div>
          <div>Open source under the MIT license. Built with Tauri, Rust, React and Monaco.</div>
        </div>
        <nav className="flex flex-wrap gap-x-5 gap-y-2">
          <Link href="/docs" className="hover:text-foreground">Docs</Link>
          <Link href="/compare" className="hover:text-foreground">Compare</Link>
          <Link href="/changelog" className="hover:text-foreground">Changelog</Link>
          <Link href="/download" className="hover:text-foreground">Download</Link>
          <a href={site.repo} className="hover:text-foreground" target="_blank" rel="noopener noreferrer">GitHub</a>
          <a href={site.issues} className="hover:text-foreground" target="_blank" rel="noopener noreferrer">Issues</a>
          <a href={site.lynkr} className="hover:text-foreground" target="_blank" rel="noopener noreferrer">Lynkr</a>
        </nav>
      </div>
    </footer>
  );
}
