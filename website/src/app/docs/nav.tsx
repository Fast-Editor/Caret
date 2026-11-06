"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";

export function DocsNav({ items }: { items: { slug: string; title: string }[] }) {
  const pathname = usePathname();
  return (
    <nav className="flex flex-col gap-0.5">
      {items.map((it) => {
        const href = it.slug ? `/docs/${it.slug}` : "/docs";
        const active = pathname === href;
        return (
          <Link key={href} href={href} className={cn("rounded-md px-2 py-1.5 text-sm transition-colors", active ? "bg-accent font-medium text-accent-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground")}>
            {it.title}
          </Link>
        );
      })}
    </nav>
  );
}
