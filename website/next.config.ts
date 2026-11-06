import type { NextConfig } from "next";

// Pure static output: deploys to Vercel's free tier (and to Cloudflare Pages or GitHub Pages unchanged).
// Rewrites and redirects live in vercel.json so they work on the static host too.
const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: false,
  images: { unoptimized: true },
  turbopack: {
    root: import.meta.dirname,
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
