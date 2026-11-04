export default defineNuxtConfig({
  compatibilityDate: "2026-11-01",
  modules: ["@nuxt/content"],
  css: ["~/assets/main.css"],
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
        toc: { depth: 3, searchDepth: 3 },
        highlight: { theme: "github-dark-default", langs: ["sh", "toml", "yaml", "json"] },
      },
    },
  },
  routeRules: {
    "/docs/workspaces-and-sessions": { redirect: "/docs/projects-and-workspaces" },
  },
  nitro: {
    prerender: { crawlLinks: true, routes: ["/", "/docs", "/sitemap.xml", "/robots.txt", "/search.json", "/docs/workspaces-and-sessions"], failOnError: true },
  },
  devtools: { enabled: false },
});
