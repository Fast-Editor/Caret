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

From the CLI (already linked as project `caret`):

```bash
npx vercel login
npx vercel deploy --prod --yes        # from the repository root
```

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
