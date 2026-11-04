// Pulpo's docs: every page is plain HTML with its content in <main>; this
// adds what they share: the top bar, the list of pages (from PAGES, the one
// place a page is listed), the page's sections, previous and next, copy
// buttons and the search (⌘K), which reads the pages listed here.

// In a block of its own: names like `top` or `open` are the window's.
{
const PAGES = [
  ["Get started", [
    ["", "Introduction"],
    ["installation", "Installation"],
    ["quick-start", "Quick start"],
  ]],
  ["Using Pulpo", [
    ["screen", "The screen"],
    ["workspaces-and-sessions", "Workspaces and sessions"],
    ["terminals", "Terminals"],
    ["files", "Files"],
    ["changes", "Changes"],
    ["agents", "Agents"],
  ]],
  ["Task sessions", [
    ["task-sessions", "Task sessions"],
    ["docker", "Docker and ports"],
  ]],
  ["Reference", [
    ["keybinds", "Keybinds"],
    ["configuration", "Configuration"],
    ["cli", "Command line"],
    ["files-on-disk", "Files on disk"],
    ["troubleshooting", "Troubleshooting"],
  ]],
];

const REPO = "https://github.com/boardchow/pulpo";
// Cargo.toml's version, shown by the name.
const VERSION = "0.1.0";

// The docs' root, from where this script is: pages link from there.
const root = new URL(".", document.currentScript.src);
const here = document.body.dataset.page ?? "";
const flat = PAGES.flatMap(([, pages]) => pages);
const href = (slug) => new URL(slug ? `${slug}/` : "./", root).href;

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === "class") node.className = value;
    else node.setAttribute(key, value);
  }
  node.append(...children);
  return node;
}

// The top bar.
const menu = el("button", { class: "menu", "aria-label": "Pages" }, "☰");
const searchButton = el("button", { class: "search-open" }, el("span", {}, "Search the docs"), el("kbd", {}, "⌘K"));
const bar = el(
  "header",
  { class: "top" },
  menu,
  el("a", { class: "brand", href: new URL("../", root).href }, el("img", { src: new URL("../favicon.svg", root).href, alt: "" }), el("span", {}, "Pulpo"), el("span", { class: "version" }, `v${VERSION}`), el("span", { class: "label" }, "Docs")),
  el("div", { class: "spacer" }),
  searchButton,
  el("a", { class: "github", href: REPO }, "GitHub"),
);

// The pages.
const nav = el("nav", { class: "nav" });
for (const [section, pages] of PAGES) {
  nav.append(el("h4", {}, section));
  for (const [slug, title] of pages) {
    nav.append(el("a", { href: href(slug), class: slug === here ? "on" : "" }, title));
  }
}
menu.addEventListener("click", () => nav.classList.toggle("open"));

// The page's sections, each heading with an anchor.
const main = document.querySelector("main");
const toc = el("aside", { class: "toc" }, el("h4", {}, "On this page"));
const slugify = (text) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
const headings = [...main.querySelectorAll("h2, h3")];
for (const heading of headings) {
  heading.id ||= slugify(heading.textContent);
  heading.append(el("a", { class: "anchor", href: `#${heading.id}`, "aria-hidden": "true" }, "#"));
  toc.append(el("a", { href: `#${heading.id}`, class: heading.tagName === "H3" ? "sub" : "" }, heading.firstChild.textContent));
}
if (headings.length === 0) toc.replaceChildren();
// The section you're reading stands out.
const tocLinks = [...toc.querySelectorAll("a")];
const onScroll = () => {
  let current = headings[0];
  for (const heading of headings) if (heading.getBoundingClientRect().top < 120) current = heading;
  tocLinks.forEach((a) => a.classList.toggle("on", current && a.hash === `#${current.id}`));
};
addEventListener("scroll", onScroll, { passive: true });

// Previous, next, and editing the page.
const at = flat.findIndex(([slug]) => slug === here);
const pager = el("div", { class: "pager" });
if (at > 0) pager.append(el("a", { href: href(flat[at - 1][0]) }, el("small", {}, "Previous"), flat[at - 1][1]));
if (at < flat.length - 1) pager.append(el("a", { class: "next", href: href(flat[at + 1][0]) }, el("small", {}, "Next"), flat[at + 1][1]));
const file = here ? `web/docs/${here}/index.html` : "web/docs/index.html";
main.append(pager, el("a", { class: "edit", href: `${REPO}/edit/main/${file}` }, "Edit this page on GitHub"));

// Copy buttons on code.
for (const pre of main.querySelectorAll("pre")) {
  const button = el("button", { class: "copy" }, "Copy");
  button.addEventListener("click", async () => {
    const text = [...pre.querySelector("code").childNodes].filter((n) => !n.classList?.contains("comment")).map((n) => n.textContent).join("");
    await navigator.clipboard.writeText(text.replace(/^\$ /gm, "").trim());
    button.textContent = "Copied";
    setTimeout(() => (button.textContent = "Copy"), 1500);
  });
  pre.append(button);
}

const layout = el("div", { class: "layout" }, nav, main, toc);
document.body.prepend(bar, layout);
onScroll();

// Search: every page's sections, read once, the first time it opens.
const input = el("input", { type: "search", placeholder: "Search the docs", "aria-label": "Search" });
const results = el("ul");
const search = el("div", { class: "search" }, el("div", { class: "box" }, input, results));
document.body.append(search);
let sections = null;
let picked = 0;

async function load() {
  const pages = await Promise.all(
    flat.map(async ([slug, title]) => {
      try {
        const html = await (await fetch(new URL(slug ? `${slug}/index.html` : "index.html", root))).text();
        const doc = new DOMParser().parseFromString(html, "text/html");
        const found = [{ slug, title, heading: "", id: "", text: "" }];
        for (const node of doc.querySelector("main")?.children ?? []) {
          if (/^H[23]$/.test(node.tagName)) found.push({ slug, title, heading: node.textContent, id: node.id || slugify(node.textContent), text: "" });
          else found[found.length - 1].text += ` ${node.textContent}`;
        }
        return found;
      } catch {
        return [];
      }
    }),
  );
  sections = pages.flat();
}

function show() {
  const words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
  results.replaceChildren();
  if (!words.length || !sections) return;
  const hits = sections
    .map((s) => {
      const haystack = `${s.title} ${s.heading} ${s.text}`.toLowerCase();
      if (!words.every((w) => haystack.includes(w))) return null;
      // Words in the title or heading come first.
      const named = `${s.title} ${s.heading}`.toLowerCase();
      return { s, score: words.filter((w) => named.includes(w)).length };
    })
    .filter(Boolean)
    .sort((a, b) => b.score - a.score)
    .slice(0, 12);
  if (!hits.length) results.append(el("li", { class: "empty" }, "Nothing found."));
  hits.forEach(({ s }, i) => {
    const link = el("a", { href: href(s.slug) + (s.id ? `#${s.id}` : ""), class: i === picked ? "on" : "" }, s.heading || s.title, el("small", {}, s.heading ? s.title : "Page"));
    results.append(el("li", {}, link));
  });
}

async function open() {
  search.classList.add("open");
  input.focus();
  input.select();
  if (!sections) {
    await load();
    show();
  }
}
const close = () => search.classList.remove("open");

searchButton.addEventListener("click", open);
search.addEventListener("click", (e) => e.target === search && close());
input.addEventListener("input", () => {
  picked = 0;
  show();
});
addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "k") {
    e.preventDefault();
    open();
  } else if (e.key === "/" && !search.classList.contains("open") && document.activeElement === document.body) {
    e.preventDefault();
    open();
  } else if (search.classList.contains("open")) {
    const links = [...results.querySelectorAll("a")];
    if (e.key === "Escape") close();
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      picked = Math.max(0, Math.min(links.length - 1, picked + (e.key === "ArrowDown" ? 1 : -1)));
      links.forEach((a, i) => a.classList.toggle("on", i === picked));
      links[picked]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && links[picked]) {
      location.href = links[picked].href;
      close();
    }
  }
});
}
