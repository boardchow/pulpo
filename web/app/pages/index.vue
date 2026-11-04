<script setup>
const description = "Spin up a few coding agents, give each its own copy of your repo, and see who needs you. One terminal. Have fun :)";
const site = useRuntimeConfig().public.site;
useSeoMeta({
  title: "Pulpo: Many agents. One terminal.",
  description,
  ogTitle: "Pulpo: Many agents. One terminal.",
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
</script>

<template>
  <div class="landing">
    <LandingField />
    <div class="page">
      <header>
        <NuxtLink class="brand" to="/"><img src="/favicon.svg" alt="">Pulpo</NuxtLink>
        <nav>
          <NuxtLink to="/docs">Docs</NuxtLink>
          <a href="https://github.com/boardchow/pulpo" target="_blank" rel="noopener">GitHub</a>
        </nav>
      </header>
      <main>
        <p class="badge"><span class="dot" />v0.1 · beta</p>
        <h1>Many agents.<br>One terminal.</h1>
        <p class="pitch">{{ description }}</p>
        <div class="install">
          <code><span class="prompt">$ </span><span>{{ command }}</span></code>
          <button type="button" @click="copy">{{ copied }}</button>
        </div>
      </main>
    </div>
  </div>
</template>

<style scoped>
.landing { overflow-x: hidden; }
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
  position: relative;
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
  /* Centered in the whole window, not just below the header, then raised 25px: a touch above center reads as centered. */
  margin-top: calc(-1 * var(--header));
  padding: calc(var(--header) + 24px) 0 calc(var(--header) + 74px);
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
@media (max-width: 560px) {
  .install { font-size: 12px; }
}
</style>
