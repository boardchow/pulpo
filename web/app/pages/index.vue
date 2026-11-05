<script setup>
const title = "Pulpo: run coding agents side by side on your Mac";
const description = "Run several coding agents at once, each in its own git worktree, branch and ports. Pulpo marks the one waiting for you. Free, open source, macOS beta.";
const site = useRuntimeConfig().public.site;
useSeoMeta({
  title,
  description,
  ogTitle: title,
  ogDescription: description,
  ogUrl: site,
  ogImage: `${site}/og.png`,
  ogImageWidth: 2400,
  ogImageHeight: 1260,
  twitterCard: "summary_large_image",
});
useHead({ link: [{ rel: "canonical", href: site }] });

const command = "curl -fsSL https://pulpo.sh/install | sh";
const copied = ref("Copy");
async function copy() {
  try {
    await navigator.clipboard.writeText(command);
    copied.value = "Copied";
  } catch {
    copied.value = "Select it";
  }
  setTimeout(() => (copied.value = "Copy"), 1600);
}

const scrolled = ref(false);
const onScroll = () => (scrolled.value = scrollY > 0);
onMounted(() => {
  onScroll();
  addEventListener("scroll", onScroll, { passive: true });
});
onBeforeUnmount(() => removeEventListener("scroll", onScroll));

// The window's bottom edge fades the screenshot, until the footer, which has its own fade, comes into view.
const footer = ref(null);
const footerSeen = ref(false);
let watching;
onMounted(() => {
  watching = new IntersectionObserver(([entry]) => (footerSeen.value = entry.isIntersecting));
  watching.observe(footer.value);
});
onBeforeUnmount(() => watching?.disconnect());

// The globe turns east, one side every 1.5s, unless motion is reduced.
const sides = ["🌍", "🌏", "🌎"];
const side = ref(0);
let turning;
onMounted(() => {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  turning = setInterval(() => (side.value = (side.value + 1) % sides.length), 1500);
});
onBeforeUnmount(() => clearInterval(turning));
</script>

<template>
  <div class="landing">
    <LandingField />
    <div class="page">
      <header :class="{ scrolled }">
        <NuxtLink class="brand" to="/"><img src="/favicon.svg" alt="">Pulpo</NuxtLink>
        <nav>
          <NuxtLink to="/docs">Docs</NuxtLink>
          <a href="https://github.com/boardchow/pulpo" target="_blank" rel="noopener">GitHub</a>
        </nav>
      </header>
      <main>
        <p class="badge"><span class="dot" />v0.1.0 · beta</p>
        <h1>Stop checking<br>every tab.</h1>
        <p class="pitch">Run several coding agents, each on its own branch and ports. Pulpo marks the one waiting for you and takes you straight to it.</p>
        <div class="install">
          <code><span class="prompt">$ </span><span>{{ command }}</span></code>
          <button type="button" @click="copy">{{ copied }}</button>
        </div>
      </main>
      <div class="shot">
        <img src="/screenshot.png" width="2560" height="1412" alt="Pulpo: projects and workspaces on the left, Claude Code in a terminal in the middle, files, changes and history on the right">
      </div>
      <div class="edge" :class="{ hidden: footerSeen }" />
      <footer ref="footer">
        <div class="meta">
          <span>Built across time zones <span class="globe" aria-hidden="true">{{ sides[side] }}</span></span>
          <nav>
            <NuxtLink to="/docs">Docs</NuxtLink>
            <a href="https://github.com/boardchow/pulpo" target="_blank" rel="noopener">GitHub</a>
            <a href="https://github.com/boardchow/pulpo/blob/main/LICENSE" target="_blank" rel="noopener">MIT license</a>
          </nav>
        </div>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.landing { overflow-x: clip; }
.landing :where(p, h1) { margin: 0; }
.page {
  position: relative;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  padding: 0 16px;
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  position: sticky;
  top: 0;
  z-index: 1;
  width: 100%;
  max-width: 1080px;
  height: var(--header);
  margin: 0 auto;
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
}
/* The window's full width, though the header itself is only as wide as the content. */
header::before {
  content: "";
  position: absolute;
  inset: 0 50%;
  width: 100vw;
  transform: translateX(-50%);
  z-index: -1;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  opacity: 0;
  transition: opacity 0.15s;
}
header.scrolled::before { opacity: 1; }
header .brand {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--fg);
  text-decoration: none;
  font-weight: 600;
  letter-spacing: 0.02em;
  text-transform: none;
  font-size: 15px;
}
header .brand img { width: 32px; height: 20px; }
a { color: inherit; }
header nav { display: flex; gap: 28px; }
header nav a { text-decoration: none; transition: color 0.15s; }
header nav a:hover { color: var(--fg); }
main {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  padding-top: clamp(24px, 9vh, 112px);
}
.badge {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 14px;
  margin-bottom: 10px;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--fg);
  background: rgba(16, 16, 18, 0.88);
  border: 1px solid var(--border);
  border-radius: 999px;
}
.badge .dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--blue);
  box-shadow: 0 0 8px var(--blue);
}
h1 {
  font-size: clamp(40px, 12.5vw, 96px);
  font-weight: 600;
  letter-spacing: -0.035em;
  line-height: 1;
  text-shadow: 0 0 40px var(--bg), 0 0 12px var(--bg);
}
.pitch {
  max-width: 520px;
  margin-top: 8px;
  font-size: clamp(16px, 2.2vw, 18px);
  line-height: 1.55;
  color: var(--muted);
  text-shadow: 0 0 16px var(--bg);
}
.install {
  display: flex;
  align-items: center;
  gap: 12px;
  max-width: 100%;
  margin-top: 36px;
  padding: 12px 12px 12px 16px;
  background: rgba(16, 16, 18, 0.88);
  border: 1px solid var(--border);
  border-radius: 8px;
  font-family: var(--mono);
  font-size: 13px;
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
}
.install code {
  overflow-x: auto;
  white-space: nowrap;
  text-align: left;
  scrollbar-width: none;
}
.install code::-webkit-scrollbar { display: none; }
.install .prompt { color: var(--dim); user-select: none; }
.install button {
  flex: none;
  font: inherit;
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 6px 10px;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
.install button:hover { color: var(--fg); border-color: var(--dim); }
.shot {
  position: relative;
  width: 100%;
  max-width: 1080px;
  margin: 56px auto 0;
}
/* Its bottom edge, and the field around it, fades into the footer. */
.shot::after {
  content: "";
  position: absolute;
  bottom: 0;
  left: 50%;
  width: 100vw;
  height: 160px;
  transform: translateX(-50%);
  background: linear-gradient(to bottom, transparent, var(--bg));
  pointer-events: none;
}
.edge {
  position: fixed;
  inset: auto 0 0;
  z-index: 1;
  height: 120px;
  background: linear-gradient(to bottom, transparent, var(--bg));
  pointer-events: none;
  transition: opacity 0.2s;
}
.edge.hidden { opacity: 0; }
footer {
  position: relative;
  z-index: 1;
  width: 100%;
  max-width: 1080px;
  margin: 0 auto;
  padding: 32px 0 40px;
  font-size: 13px;
  color: var(--dim);
}
/* Solid across the window, so the field stops where the screenshot fades out. */
footer::before {
  content: "";
  position: absolute;
  inset: 0 50%;
  width: 100vw;
  transform: translateX(-50%);
  z-index: -1;
  background: var(--bg);
}
footer .meta { display: flex; justify-content: space-between; flex-wrap: wrap; gap: 8px 24px; }
footer nav { display: flex; gap: 24px; }
footer a { color: var(--muted); text-decoration: none; transition: color 0.15s; }
footer a:hover { color: var(--fg); }
footer .globe { display: inline-block; margin-left: 4px; }
.shot img {
  display: block;
  width: 100%;
  height: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
}
@media (max-width: 560px) {
  .install { font-size: 12px; }
  footer .meta { flex-direction: column-reverse; align-items: center; gap: 12px; }
  /* Readable size, starting at the left margin and cut by the window's right edge. */
  .shot { width: 800px; max-width: none; margin-left: 0; }
  .shot::after { left: -16px; transform: none; }
}
</style>
