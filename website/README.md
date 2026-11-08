# Caret website

The marketing and documentation site for Caret: Next.js (App Router, static export), Tailwind v4 and shadcn/ui.
Documentation pages are rendered at build time from the repository's `../docs/*.md`, the changelog from
`../CHANGELOG.md`, and download buttons resolve the latest GitHub release, so the site needs no CMS and no server.

## Develop

```bash
cd website
npm install
npm run dev        # http://localhost:3000
npm run build      # static export in ./out
```

Set `GITHUB_TOKEN` when building often: the release lookup uses the GitHub API (60 unauthenticated requests per hour),
and falls back to the releases page when it is unavailable. `NEXT_PUBLIC_SITE_URL` sets the canonical URL used in
metadata (defaults to the Vercel address).

## Deploy to Vercel (free Hobby tier)

The docs pages read `../docs` and `../CHANGELOG.md`, so the deployment root is the **repository root**, not `website/`.
The root `vercel.json` installs and builds inside `website/` and serves `website/out`; it also adds `/install.sh`
(rewrites to the install script on GitHub so `curl -fsSL https://<domain>/install.sh | sh` works), `/latest.json`, and
short redirects for `/github`, `/releases`, `/issues` and `/download/latest`. `.vercelignore` keeps the Rust build
directories out of the upload.

From the CLI (already linked as project `caret` in the `lynkrteam` scope): Vercel reads the local commit author and
blocks deployments whose author is not a member of the team, so deploy from a copy of the checkout without `.git`:

```bash
npx vercel login
rsync -a --exclude .git --exclude node_modules --exclude 'website/node_modules' --exclude 'website/.next' \
  --exclude 'website/out' --exclude 'src-tauri/target*' ./ /tmp/caret-deploy/
mkdir -p /tmp/caret-deploy/.vercel && cp .vercel/project.json /tmp/caret-deploy/.vercel/
npx vercel deploy --prod --yes --scope lynkrteam --cwd /tmp/caret-deploy
```

Production: https://caret-lynkrteam.vercel.app (also https://caret-ochre.vercel.app). Deployment Protection was turned
off for the project so the site is public.

For automatic deploys on push, open the project in the Vercel dashboard → **Settings → Git** and connect
`Fast-Editor/Caret`; leave **Root Directory** empty (the root `vercel.json` handles the build). A custom domain goes
under **Settings → Domains** (CNAME to `cname.vercel-dns.com`); then set `NEXT_PUBLIC_SITE_URL` under
**Settings → Environment Variables** so metadata and social cards use it.

The output is plain static files, so the same `website/out` directory also deploys to Cloudflare Pages or GitHub Pages;
only the rewrites in `vercel.json` are Vercel-specific.

## Structure

- `src/app/page.tsx` landing page; `src/lib/site.ts` holds the copy, feature list, comparison table and FAQ.
- `src/app/docs/*` documentation index and pages; `src/lib/docs.ts` reads the markdown, `src/lib/markdown.ts` renders it
  (remark + rehype, Shiki highlighting, repository links rewritten to site routes).
- `src/app/download`, `src/app/changelog`.
- `src/components/site/*` header, footer, theme toggle, install command, download grid; `src/components/ui/*` shadcn.

## SEO

`src/lib/seo.ts` (`pageMeta`) builds the full per-page metadata: title, description, canonical URL, Open Graph and Twitter
card (`public/og.png`, 1200x630, rendered from `/tmp/og.html`-style HTML with headless Chrome). `src/app/robots.ts` and
`src/app/sitemap.ts` are static routes (every page plus one entry per doc, `lastModified` from file mtimes). Structured
data: the home page emits Organization, WebSite, SoftwareApplication and FAQPage JSON-LD; doc pages emit TechArticle and
BreadcrumbList; the compare page a BreadcrumbList. `public/llms.txt` summarises the project for AI crawlers,
`public/manifest.webmanifest`, `favicon.ico` and `apple-touch-icon.png` are generated from `public/icon.png`
(`magick icon.png -define icon:auto-resize=16,32,48 favicon.ico`). The canonical host is `site.url`
(`NEXT_PUBLIC_SITE_URL`); the root `vercel.json` redirects the secondary `caret-lynkrteam.vercel.app` alias to it. Set
`NEXT_PUBLIC_GOOGLE_SITE_VERIFICATION` in Vercel to add the Search Console meta tag.
