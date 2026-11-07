import Link from "next/link";
import { ArrowRight, Download } from "lucide-react";
import { GithubIcon } from "@/components/site/github-icon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion";
import { Separator } from "@/components/ui/separator";
import { InstallCommand } from "@/components/site/install-command";
import { DownloadGrid } from "@/components/site/download-grid";
import { FeatureIcon } from "@/components/site/feature-icon";
import { getLatestRelease } from "@/lib/release";
import { comparison, faq, features, site, stats } from "@/lib/site";

export default async function Home() {
  const release = await getLatestRelease();
  const mac = release.downloads.macArm;
  return (
    <div className="mx-auto max-w-6xl px-4 sm:px-6">
      {/* Hero */}
      <section className="flex flex-col items-center gap-6 pb-16 pt-20 text-center sm:pt-28">
        <Badge variant="secondary" className="gap-2 rounded-full px-3 py-1 font-normal">
          <span className="size-1.5 rounded-full bg-emerald-500" /> {release.tag === "latest" ? "Latest release" : release.tag} · macOS, Windows, Linux · MIT
        </Badge>
        <h1 className="max-w-3xl text-balance text-4xl font-semibold tracking-tight sm:text-6xl">{site.tagline}</h1>
        <p className="max-w-2xl text-balance text-lg text-muted-foreground">{site.description}</p>
        <div className="flex flex-wrap items-center justify-center gap-3">
          <Button size="lg" render={<a href={mac?.url ?? site.latest} />}>
            <Download className="size-4" /> Download for macOS
          </Button>
          <Button size="lg" variant="outline" render={<a href={site.repo} target="_blank" rel="noopener noreferrer" />}>
            <GithubIcon className="size-4" /> Star on GitHub
          </Button>
          <Button size="lg" variant="ghost" render={<Link href="/docs" />}>
            Read the docs <ArrowRight className="size-4" />
          </Button>
        </div>
        <InstallCommand command={site.installCommand} className="w-full max-w-2xl text-left" />
        <p className="text-sm text-muted-foreground">
          Coming from T3 Code or Cursor? <Link href="/compare" className="underline underline-offset-4 hover:text-foreground">See the feature comparison</Link>.
        </p>
        <a href={site.productHunt} target="_blank" rel="noopener noreferrer" className="mt-2 opacity-90 transition-opacity hover:opacity-100">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img alt="Caret on Product Hunt" width={250} height={54} src={site.productHuntBadge} />
        </a>
      </section>

      {/* Stats */}
      <section className="grid grid-cols-2 gap-3 pb-20 md:grid-cols-4">
        {stats.map((s) => (
          <Card key={s.label} className="py-5">
            <CardContent className="flex flex-col gap-1 px-5">
              <div className="font-mono text-2xl font-semibold tracking-tight">{s.value}</div>
              <div className="text-sm text-muted-foreground">{s.label}</div>
            </CardContent>
          </Card>
        ))}
      </section>

      {/* Why */}
      <section id="why" className="pb-20">
        <div className="mb-8 max-w-2xl">
          <h2 className="text-3xl font-semibold tracking-tight">Why Caret</h2>
          <p className="mt-3 text-muted-foreground">
            Cursor proved what an AI-native editor should feel like. Caret keeps that feel and swaps the runtime: one Rust process and the operating system&apos;s WebView instead of a bundled browser and Node extension hosts.
          </p>
        </div>
        <Card className="overflow-hidden py-0">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left text-muted-foreground">
              <tr><th className="px-5 py-3 font-medium">Area</th><th className="px-5 py-3 font-medium">Electron editors</th><th className="px-5 py-3 font-medium text-foreground">Caret</th></tr>
            </thead>
            <tbody>
              {comparison.map((row) => (
                <tr key={row.aspect} className="border-t border-border/60">
                  <td className="px-5 py-3 font-medium">{row.aspect}</td>
                  <td className="px-5 py-3 text-muted-foreground">{row.electron}</td>
                  <td className="px-5 py-3">{row.caret}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <p className="mt-3 text-xs text-muted-foreground">Numbers measured on the release build; method and details in <Link href="/docs/performance" className="underline underline-offset-4">Performance</Link>.</p>
      </section>

      {/* Features */}
      <section id="features" className="pb-20">
        <div className="mb-8 max-w-2xl">
          <h2 className="text-3xl font-semibold tracking-tight">Everything the agent needs, in one process</h2>
          <p className="mt-3 text-muted-foreground">Completions, inline edits, an agent with real tools, a code index, MCP, external agents, a debugger and a safety layer, all talking to the same Rust core.</p>
        </div>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {features.map((f) => (
            <Card key={f.title} className="gap-3 py-5">
              <CardHeader className="px-5">
                <div className="mb-1 flex size-9 items-center justify-center rounded-lg border border-border bg-muted/40"><FeatureIcon name={f.icon} className="size-4" /></div>
                <CardTitle className="text-base">{f.title}</CardTitle>
                <CardDescription className="leading-relaxed">{f.body}</CardDescription>
              </CardHeader>
            </Card>
          ))}
        </div>
      </section>

      {/* Agents */}
      <section id="agents" className="pb-20">
        <Card className="bg-muted/30 py-8">
          <CardContent className="grid gap-8 px-6 md:grid-cols-2">
            <div>
              <h2 className="text-2xl font-semibold tracking-tight">Bring the agent you already pay for</h2>
              <p className="mt-3 text-muted-foreground">
                Pick Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode, Pi or any Agent Client Protocol agent per chat. The agent runs locally with its own login; its streaming text, tool calls, permission prompts, checkpointed diffs and terminals all flow through Caret&apos;s review bar and approval cards.
              </p>
              <Button variant="link" className="mt-2 px-0" render={<Link href="/docs/acp" />}>How external agents work <ArrowRight className="size-4" /></Button>
            </div>
            <div>
              <h2 className="text-2xl font-semibold tracking-tight">Or route among your own models</h2>
              <p className="mt-3 text-muted-foreground">
                Auto mode runs a managed <a className="underline underline-offset-4" href={site.lynkr} target="_blank" rel="noopener noreferrer">Lynkr</a> sidecar that scores every request and picks one of your configured models per tier, from simple to reasoning, with session pins and fallback. Keys stay in Caret; every reply shows which model answered.
              </p>
              <Button variant="link" className="mt-2 px-0" render={<Link href="/docs/auto-routing" />}>Auto routing <ArrowRight className="size-4" /></Button>
            </div>
          </CardContent>
        </Card>
      </section>

      {/* Download */}
      <section id="download" className="pb-20">
        <div className="mb-8 max-w-2xl">
          <h2 className="text-3xl font-semibold tracking-tight">Download</h2>
          <p className="mt-3 text-muted-foreground">Installers for every platform are built by CI from the release tag. After the first install, updates arrive from inside the app.</p>
        </div>
        <DownloadGrid release={release} />
      </section>

      <Separator />

      {/* FAQ */}
      <section id="faq" className="py-20">
        <div className="mb-8 max-w-2xl">
          <h2 className="text-3xl font-semibold tracking-tight">Questions</h2>
        </div>
        <Accordion className="max-w-3xl">
          {faq.map((item, i) => (
            <AccordionItem key={item.q} value={`q${i}`}>
              <AccordionTrigger className="text-left">{item.q}</AccordionTrigger>
              <AccordionContent className="text-muted-foreground">{item.a}</AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </section>
    </div>
  );
}
