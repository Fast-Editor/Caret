import type { Metadata } from "next";
import { pageMeta } from "@/lib/seo";
import { DownloadGrid } from "@/components/site/download-grid";
import { getLatestRelease } from "@/lib/release";

export const metadata: Metadata = pageMeta({ title: "Download", description: "Download Caret for macOS (Apple Silicon and Intel), Windows and Linux: DMG, MSI, deb, rpm, AppImage, Homebrew, winget, scoop and Nix.", path: "/download" });

export default async function DownloadPage() {
  const release = await getLatestRelease();
  return (
    <div className="mx-auto max-w-4xl px-4 py-10 sm:px-6">
      <h1 className="text-3xl font-semibold tracking-tight">Download Caret</h1>
      <p className="mt-3 text-muted-foreground">Installers are built by CI from the release tag. After the first install, Caret updates itself from Settings → About.</p>
      <div className="mt-8"><DownloadGrid release={release} /></div>
    </div>
  );
}
