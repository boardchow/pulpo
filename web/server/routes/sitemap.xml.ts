export default defineEventHandler(async (event) => {
  const site = useRuntimeConfig(event).public.site;
  const pages = await queryCollection(event, "docs").select("path").all();
  const urls = ["/", ...pages.map((p) => p.path)].map((path) => `  <url><loc>${site}${path === "/" ? "" : path}</loc></url>`);
  setHeader(event, "content-type", "application/xml");
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.join("\n")}\n</urlset>\n`;
});
