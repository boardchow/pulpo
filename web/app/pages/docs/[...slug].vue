<script setup>
const SECTIONS = ["Get started", "Using Pulpo", "Reference"];
const REPO = "https://github.com/boardchow/pulpo";

const route = useRoute();
// /docs/installation/ and /docs/installation are the same page.
const path = route.path.replace(/\/$/, "") || "/docs";

const { data: page } = await useAsyncData(`docs:${path}`, () => queryCollection("docs").path(path).first());
if (!page.value) {
  throw createError({ statusCode: 404, statusMessage: "No such page", fatal: true });
}
const { data: pages } = await useAsyncData("docs:pages", () =>
  queryCollection("docs").select("path", "title", "section", "order").order("order", "ASC").all(),
);

const groups = computed(() => SECTIONS.map((section) => ({ section, pages: (pages.value ?? []).filter((p) => p.section === section) })));
const flat = computed(() => groups.value.flatMap((g) => g.pages));
const at = computed(() => flat.value.findIndex((p) => p.path === path));
const previous = computed(() => flat.value[at.value - 1]);
const next = computed(() => flat.value[at.value + 1]);
const toc = computed(() => page.value?.body?.toc?.links ?? []);

const site = useRuntimeConfig().public.site;
const title = path === "/docs" ? "Pulpo docs" : `${page.value.title} · Pulpo docs`;
useSeoMeta({
  title,
  description: page.value.description,
  ogTitle: title,
  ogDescription: page.value.description,
  ogUrl: `${site}${path}`,
  ogImage: `${site}/og.png`,
  twitterCard: "summary_large_image",
});
useHead({ link: [{ rel: "canonical", href: `${site}${path}` }] });

const menuOpen = ref(false);
watch(() => route.path, () => (menuOpen.value = false));

const reading = ref("");
function onScroll() {
  const ids = toc.value.flatMap((l) => [l.id, ...(l.children ?? []).map((c) => c.id)]);
  let current = ids[0] ?? "";
  for (const id of ids) {
    const heading = document.getElementById(id);
    if (heading && heading.getBoundingClientRect().top < 120) current = id;
  }
  reading.value = current;
}
onMounted(() => {
  addEventListener("scroll", onScroll, { passive: true });
  onScroll();
});
onBeforeUnmount(() => removeEventListener("scroll", onScroll));
</script>

<template>
  <div class="docs">
    <DocsBar @menu="menuOpen = !menuOpen" />
    <div class="layout">
      <nav class="nav" :class="{ open: menuOpen }">
        <template v-for="group in groups" :key="group.section">
          <h4>{{ group.section }}</h4>
          <NuxtLink v-for="p in group.pages" :key="p.path" :to="p.path" :class="{ on: p.path === path }">{{ p.title }}</NuxtLink>
        </template>
      </nav>

      <main class="doc">
        <p class="eyebrow">{{ page.section }}</p>
        <h1>{{ page.title }}</h1>
        <p class="lead">{{ page.description }}</p>
        <ContentRenderer :value="page" />
        <div class="pager">
          <NuxtLink v-if="previous" :to="previous.path"><small>Previous</small>{{ previous.title }}</NuxtLink>
          <NuxtLink v-if="next" class="next" :to="next.path"><small>Next</small>{{ next.title }}</NuxtLink>
        </div>
        <a class="edit" :href="`${REPO}/edit/main/web/content/${page.stem}.md`">Edit this page on GitHub</a>
      </main>

      <aside class="toc">
        <template v-if="toc.length">
          <h4>On this page</h4>
          <template v-for="link in toc" :key="link.id">
            <a :href="`#${link.id}`" :class="{ on: reading === link.id }">{{ link.text }}</a>
            <a v-for="child in link.children ?? []" :key="child.id" :href="`#${child.id}`" class="sub" :class="{ on: reading === child.id }">{{ child.text }}</a>
          </template>
        </template>
      </aside>
    </div>
  </div>
</template>
