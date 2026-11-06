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

1. In Vercel, **Add New → Project**, import `Fast-Editor/Caret`.
2. Set **Root Directory** to `website`. Framework preset: Next.js (detected). Build command `npm run build`, output `out`.
3. Deploy. Every push to `main` redeploys; pull requests get preview URLs.
4. Optional: add a custom domain under **Settings → Domains** and point a CNAME at `cname.vercel-dns.com`, then set
   `NEXT_PUBLIC_SITE_URL` in **Settings → Environment Variables** to that domain.

`vercel.json` adds `/install.sh` (rewrites to the install script on GitHub so `curl -fsSL https://<domain>/install.sh | sh`
works), `/latest.json`, and short redirects for `/github`, `/releases`, `/issues` and `/download/latest`.

From the CLI instead: `npx vercel login`, then `npx vercel --cwd website` (first run links the project) and
`npx vercel --cwd website --prod`.

The output is plain static files, so the same `out/` directory also deploys to Cloudflare Pages or GitHub Pages; only the
rewrites in `vercel.json` are Vercel-specific.

## Structure

- `src/app/page.tsx` landing page; `src/lib/site.ts` holds the copy, feature list, comparison table and FAQ.
- `src/app/docs/*` documentation index and pages; `src/lib/docs.ts` reads the markdown, `src/lib/markdown.ts` renders it
  (remark + rehype, Shiki highlighting, repository links rewritten to site routes).
- `src/app/download`, `src/app/changelog`.
- `src/components/site/*` header, footer, theme toggle, install command, download grid; `src/components/ui/*` shadcn.
