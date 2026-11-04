// pulpo.sh: the landing page and the docs, generated as static HTML
// (`npm run generate`), so every page is complete for search engines and
// served from any static host. The docs are Markdown, in content/docs.
export default defineNuxtConfig({
  compatibilityDate: "2026-11-01",
  modules: ["@nuxt/content"],
  css: ["~/assets/main.css"],
  // Rendered on the server at build time, then hydrated.
  ssr: true,
  app: {
    head: {
      htmlAttrs: { lang: "en" },
      meta: [
        { name: "viewport", content: "width=device-width, initial-scale=1" },
        { name: "theme-color", content: "#090909" },
      ],
      link: [{ rel: "icon", href: "/favicon.svg", type: "image/svg+xml" }],
    },
  },
  runtimeConfig: { public: { site: "https://pulpo.sh" } },
  content: {
    // Node's own SQLite, for the docs' index at build time.
    experimental: { sqliteConnector: "native" },
    build: {
      markdown: {
        // The page's sections, for "On this page".
        toc: { depth: 3, searchDepth: 3 },
        highlight: { theme: "github-dark-default", langs: ["sh", "toml", "yaml", "json"] },
      },
    },
  },
  nitro: {
    // Every page found by following links from these, as HTML files.
    prerender: { crawlLinks: true, routes: ["/", "/docs", "/sitemap.xml", "/robots.txt", "/search.json"], failOnError: true },
  },
  devtools: { enabled: false },
});
