import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { ThemeProvider } from "@/components/site/theme-provider";
import { Header } from "@/components/site/header";
import { Footer } from "@/components/site/footer";
import { site } from "@/lib/site";
import { pageMeta } from "@/lib/seo";

const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"] });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

const home = pageMeta({ description: site.seoDescription, path: "/" });

export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: { default: site.seoTitle, template: `%s · ${site.name}` },
  description: site.seoDescription,
  applicationName: site.name,
  keywords: ["AI code editor", "open source Cursor alternative", "Rust code editor", "Tauri", "Claude Code", "Codex", "Agent Client Protocol", "local LLM editor", "Ollama"],
  authors: [{ name: "Caret contributors", url: site.repo }],
  creator: "Caret contributors",
  manifest: "/manifest.webmanifest",
  icons: { icon: [{ url: "/favicon.ico", sizes: "48x48" }, { url: "/icon.png", type: "image/png", sizes: "512x512" }], apple: "/apple-touch-icon.png" },
  robots: { index: true, follow: true, googleBot: { index: true, follow: true, "max-image-preview": "large", "max-snippet": -1 } },
  // Search Console HTML-tag verification. The token is public (it ships in the page head); the env var lets a fork override it.
  verification: { google: process.env.NEXT_PUBLIC_GOOGLE_SITE_VERIFICATION ?? "jynfImKAjk_mqiBYUOb0cG17HydMoKjP-U6-WJ-4cC4" },
  // Home page share card and canonical; `title` keeps the template so child pages get the " · Caret" suffix.
  alternates: home.alternates, openGraph: home.openGraph, twitter: home.twitter,
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" suppressHydrationWarning className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}>
      <body className="min-h-full flex flex-col bg-background text-foreground">
        <ThemeProvider>
          <Header />
          <main className="flex-1">{children}</main>
          <Footer />
        </ThemeProvider>
      </body>
    </html>
  );
}
