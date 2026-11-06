import Link from "next/link";
import Image from "next/image";
import { Menu } from "lucide-react";
import { GithubIcon } from "./github-icon";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { nav, site } from "@/lib/site";
import { ThemeToggle } from "./theme-toggle";

export function Header() {
  return (
    <header className="sticky top-0 z-40 border-b border-border/60 bg-background/80 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="mx-auto flex h-14 max-w-6xl items-center gap-4 px-4 sm:px-6">
        <Link href="/" className="flex items-center gap-2 font-semibold tracking-tight">
          <Image src="/caret.png" alt="" width={24} height={24} className="rounded-md" priority />
          <span>Caret</span>
        </Link>
        <nav className="ml-6 hidden items-center gap-1 md:flex">
          {nav.map((n) => (
            <Button key={n.href} variant="ghost" size="sm" render={<Link href={n.href} />}>{n.label}</Button>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="sm" className="hidden sm:inline-flex" render={<a href={site.repo} target="_blank" rel="noopener noreferrer" />}>
            <GithubIcon className="size-4" /> GitHub
          </Button>
          <ThemeToggle />
          <Sheet>
            <SheetTrigger render={<Button variant="ghost" size="icon" className="md:hidden" aria-label="Open menu" />}>
              <Menu className="size-4" />
            </SheetTrigger>
            <SheetContent side="right" className="w-72">
              <SheetTitle className="sr-only">Navigation</SheetTitle>
              <div className="mt-8 flex flex-col gap-1">
                {nav.map((n) => (
                  <Button key={n.href} variant="ghost" className="justify-start" render={<Link href={n.href} />}>{n.label}</Button>
                ))}
                <Button variant="ghost" className="justify-start" render={<a href={site.repo} target="_blank" rel="noopener noreferrer" />}>
                  <GithubIcon className="size-4" /> GitHub
                </Button>
              </div>
            </SheetContent>
          </Sheet>
        </div>
      </div>
    </header>
  );
}
