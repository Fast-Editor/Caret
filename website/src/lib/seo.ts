import type { Metadata } from "next";
import { site } from "./site";

export const OG_IMAGE = { url: "/og.png", width: 1200, height: 630, alt: `${site.name}: ${site.tagline}` };

/**
 * Complete per-page metadata. Next does not merge nested `openGraph`/`twitter` objects with the root layout, so every
 * page builds the full set here: title, description, canonical URL, Open Graph and Twitter card.
 */
export function pageMeta(opts: { title?: string; description: string; path: string; type?: "website" | "article"; modified?: Date }): Metadata {
  const url = new URL(opts.path, site.url).toString();
  const fullTitle = opts.title ? `${opts.title} · ${site.name}` : site.seoTitle;
  return {
    title: opts.title ?? { absolute: site.seoTitle },
    description: opts.description,
    alternates: { canonical: url },
    openGraph: {
      title: fullTitle, description: opts.description, url, siteName: site.name, locale: "en_US",
      type: opts.type ?? "website", images: [OG_IMAGE],
      ...(opts.type === "article" && opts.modified ? { modifiedTime: opts.modified.toISOString() } : {}),
    },
    twitter: { card: "summary_large_image", site: site.twitter, title: fullTitle, description: opts.description, images: [OG_IMAGE.url] },
  };
}

/** Markdown inline code/emphasis markers removed, for plain-text fields in structured data. */
export function plain(text: string): string { return text.replace(/[`*_]/g, "").replace(/\s+/g, " ").trim(); }
