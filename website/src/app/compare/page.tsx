import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, Check, Download, Minus, X, type LucideIcon } from "lucide-react";
import { GithubIcon } from "@/components/site/github-icon";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { parity, site, stats, type ParityRow, type ParityStatus } from "@/lib/site";
import { cn } from "@/lib/utils";

export const metadata: Metadata = {
  title: "Compare",
  description: "Feature by feature: Caret next to T3 Code and Cursor. Editor, agents and subscriptions, threads, safety, remote access and footprint.",
};

const STATUS: Record<ParityStatus, { label: string; icon: LucideIcon; className: string }> = {
  yes: { label: "Yes", icon: Check, className: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400" },
  partial: { label: "Partial", icon: Minus, className: "bg-amber-500/10 text-amber-700 dark:text-amber-400" },
  no: { label: "No", icon: X, className: "bg-muted text-muted-foreground" },
};

function Status({ value }: { value: ParityStatus }) {
  const s = STATUS[value];
  const Icon = s.icon;
  return (
    <span className={cn("inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium", s.className)}>
      <Icon className="size-3" aria-hidden="true" />
      {s.label}
    </span>
  );
}

const COLUMNS: { key: "caret" | "t3" | "cursor"; label: string }[] = [
  { key: "caret", label: "Caret" },
  { key: "t3", label: "T3 Code" },
  { key: "cursor", label: "Cursor" },
];

function ParityTable({ group, rows }: { group: string; rows: ParityRow[] }) {
  return (
    <section aria-labelledby={`group-${group.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`}>
      <h2 id={`group-${group.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`} className="mb-3 text-xl font-semibold tracking-tight">{group}</h2>
      <Card className="overflow-hidden py-0">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left text-muted-foreground">
              <tr>
                <th className="px-5 py-3 font-medium">Feature</th>
                {COLUMNS.map((c) => (
                  <th key={c.key} className={cn("w-28 px-5 py-3 font-medium", c.key === "caret" && "text-foreground")}>{c.label}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.feature} className="border-t border-border/60">
                  <td className="px-5 py-3">
                    <div className="font-medium">{row.feature}</div>
                    {row.note && <div className="mt-0.5 text-xs text-muted-foreground">{row.note}</div>}
                  </td>
                  {COLUMNS.map((c) => (
                    <td key={c.key} className="px-5 py-3 align-top"><Status value={row[c.key]} /></td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </section>
  );
}

export default function ComparePage() {
  const groups = Array.from(new Set(parity.map((r) => r.group)));
  return (
    <div className="mx-auto max-w-6xl px-4 py-10 sm:px-6">
      <div className="max-w-3xl">
        <h1 className="text-3xl font-semibold tracking-tight">Caret, T3 Code and Cursor</h1>
        <p className="mt-3 text-muted-foreground">
          Three different shapes. Caret is an editor with the agent layer built in: one Rust process, the system WebView, your own keys and subscriptions.
          T3 Code is an open-source control surface for coding agent harnesses such as Claude Code and Codex, with threads and worktrees but no editor by design.
          Cursor is an Electron editor whose agent runs through Cursor&apos;s backend. The tables below mark what each one ships today.
        </p>
        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted-foreground">
          <span>Legend:</span>
          <span className="inline-flex items-center gap-1.5"><Status value="yes" /> shipped</span>
          <span className="inline-flex items-center gap-1.5"><Status value="partial" /> partial, or only in some plans</span>
          <span className="inline-flex items-center gap-1.5"><Status value="no" /> not available</span>
        </div>
      </div>

      <div className="mt-10 flex flex-col gap-10">
        {groups.map((g) => (
          <ParityTable key={g} group={g} rows={parity.filter((r) => r.group === g)} />
        ))}
      </div>

      <p className="mt-4 text-xs text-muted-foreground">
        Rows describe shipped behaviour as of November 2026. If one is out of date, <a className="underline underline-offset-4" href={site.issues} target="_blank" rel="noopener noreferrer">open an issue</a>.
      </p>

      {/* Measured */}
      <section className="mt-16" aria-labelledby="measured">
        <div className="max-w-3xl">
          <h2 id="measured" className="text-2xl font-semibold tracking-tight">Measured on a release build</h2>
          <p className="mt-3 text-muted-foreground">Apple Silicon Mac, release bundle, demo workspace. The method and the full tables are in the performance doc.</p>
        </div>
        <div className="mt-6 grid grid-cols-2 gap-3 md:grid-cols-4">
          {stats.map((s) => (
            <Card key={s.label} className="py-5">
              <CardContent className="flex flex-col gap-1 px-5">
                <div className="font-mono text-2xl font-semibold tracking-tight">{s.value}</div>
                <div className="text-sm text-muted-foreground">{s.label}</div>
              </CardContent>
            </Card>
          ))}
        </div>
        <div className="mt-6 max-w-3xl space-y-3 text-sm text-muted-foreground">
          <p>
            The whole process family (Rust core plus the WebKit helper processes) with a TypeScript file and a terminal open is about 490 MB, versus about 393 MB for Cursor idle with one workspace open on the same machine. The remaining gap is the WebKit content process, where Monaco, xterm and React live.
          </p>
          <p>
            T3 Code is an Electron app. We have not measured it, so this page makes no claim about its footprint. The steps in{" "}
            <Link href="/docs/performance" className="underline underline-offset-4 hover:text-foreground">Performance</Link> work for any desktop app: launch it, list its process family with <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">ps</code>, and add up the RSS column.
          </p>
        </div>
      </section>

      {/* CTA */}
      <section className="mt-16">
        <Card className="bg-muted/30 py-8">
          <CardContent className="flex flex-col items-start gap-4 px-6 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h2 className="text-xl font-semibold tracking-tight">Try it with the agent you already pay for</h2>
              <p className="mt-1 text-sm text-muted-foreground">macOS, Windows and Linux builds on every release. MIT licensed.</p>
            </div>
            <div className="flex flex-wrap gap-3">
              <Button render={<Link href="/download" />}>
                <Download className="size-4" /> Download
              </Button>
              <Button variant="outline" render={<a href={site.repo} target="_blank" rel="noopener noreferrer" />}>
                <GithubIcon className="size-4" /> GitHub <ArrowRight className="size-4" />
              </Button>
            </div>
          </CardContent>
        </Card>
      </section>
    </div>
  );
}
