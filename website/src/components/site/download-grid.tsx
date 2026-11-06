import { Apple, Download, ExternalLink, MonitorSmartphone, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatSize, type Release, type ReleaseAsset } from "@/lib/release";
import { site } from "@/lib/site";
import { InstallCommand } from "./install-command";

function AssetButton({ asset, label, primary }: { asset?: ReleaseAsset; label: string; primary?: boolean }) {
  if (!asset) {
    return (
      <Button variant={primary ? "default" : "outline"} render={<a href={site.latest} target="_blank" rel="noopener noreferrer" />}>
        {label} <ExternalLink className="size-3.5" />
      </Button>
    );
  }
  return (
    <Button variant={primary ? "default" : "outline"} render={<a href={asset.url} />}>
      <Download className="size-4" /> {label}<span className="ml-1 text-xs opacity-70">{formatSize(asset.size)}</span>
    </Button>
  );
}

export function DownloadGrid({ release }: { release: Release }) {
  const d = release.downloads;
  return (
    <Tabs defaultValue="mac" className="w-full">
      <TabsList className="grid w-full max-w-md grid-cols-3">
        <TabsTrigger value="mac"><Apple className="size-4" /> macOS</TabsTrigger>
        <TabsTrigger value="windows"><MonitorSmartphone className="size-4" /> Windows</TabsTrigger>
        <TabsTrigger value="linux"><Terminal className="size-4" /> Linux</TabsTrigger>
      </TabsList>

      <TabsContent value="mac" className="mt-4">
        <Card>
          <CardHeader>
            <CardTitle>macOS 12 or later</CardTitle>
            <CardDescription>One line installs the right build and clears the quarantine flag, so there is no Gatekeeper dialog. Updates then arrive from inside the app.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <InstallCommand command={site.installCommand} />
            <div className="text-sm text-muted-foreground">Homebrew 7 or later: <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">{site.brewCommand}</code></div>
            <div className="flex flex-wrap gap-2">
              <AssetButton asset={d.macArm} label="Apple Silicon .dmg" primary />
              <AssetButton asset={d.macX64} label="Intel .dmg" />
            </div>
            <p className="text-xs text-muted-foreground">
              Builds are ad-hoc signed until a Developer ID is configured. A .dmg downloaded in a browser is quarantined, so macOS says the app is damaged: run{" "}
              <code className="rounded bg-muted px-1 py-0.5 font-mono">{site.quarantineCommand}</code> once, or choose <em>Open Anyway</em> under Privacy &amp; Security.
            </p>
          </CardContent>
        </Card>
      </TabsContent>

      <TabsContent value="windows" className="mt-4">
        <Card>
          <CardHeader>
            <CardTitle>Windows 10 or later, x64</CardTitle>
            <CardDescription>Built by the same release workflow as macOS but with far less runtime testing. The installer is unsigned, so SmartScreen shows <em>More info → Run anyway</em> once.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            <AssetButton asset={d.winExe} label="Setup .exe" primary />
            <AssetButton asset={d.winMsi} label=".msi" />
          </CardContent>
        </Card>
      </TabsContent>

      <TabsContent value="linux" className="mt-4">
        <Card>
          <CardHeader>
            <CardTitle>Linux, x86_64</CardTitle>
            <CardDescription>Needs WebKitGTK 4.1. Sandboxing uses bubblewrap when it is installed. Report rough edges on GitHub.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            <AssetButton asset={d.deb} label=".deb" primary />
            <AssetButton asset={d.rpm} label=".rpm" />
            <AssetButton asset={d.appImage} label=".AppImage" />
          </CardContent>
        </Card>
      </TabsContent>

      <p className="mt-4 text-xs text-muted-foreground">
        Release {release.tag}{release.publishedAt ? `, published ${new Date(release.publishedAt).toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" })}` : ""}. Checksums and signatures are in <a className="underline underline-offset-4" href={release.htmlUrl} target="_blank" rel="noopener noreferrer">the release notes</a>.
      </p>
    </Tabs>
  );
}
