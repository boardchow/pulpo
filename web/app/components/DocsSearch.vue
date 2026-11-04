<script setup>
const open = ref(false);
const query = ref("");
const picked = ref(0);
const sections = ref(null);
const input = ref(null);

const hits = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length || !sections.value) return [];
  return sections.value
    .map((s) => {
      const named = [...s.titles, s.title].join(" ").toLowerCase();
      if (!words.every((w) => `${named} ${s.content}`.toLowerCase().includes(w))) return null;
      return { s, score: words.filter((w) => named.includes(w)).length };
    })
    .filter(Boolean)
    .sort((a, b) => b.score - a.score)
    .slice(0, 12)
    .map(({ s }) => ({ to: s.id, title: s.title, page: s.titles[0] ?? "Page" }));
});

watch(query, () => (picked.value = 0));

async function show() {
  open.value = true;
  await nextTick();
  input.value?.focus();
  input.value?.select();
  sections.value ??= await $fetch("/search.json");
}

function go(to) {
  open.value = false;
  navigateTo(to);
}

function onKey(e) {
  if ((e.metaKey || e.ctrlKey) && e.key === "k") {
    e.preventDefault();
    show();
  } else if (e.key === "/" && !open.value && document.activeElement === document.body) {
    e.preventDefault();
    show();
  } else if (open.value) {
    if (e.key === "Escape") open.value = false;
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      picked.value = Math.max(0, Math.min(hits.value.length - 1, picked.value + (e.key === "ArrowDown" ? 1 : -1)));
    } else if (e.key === "Enter" && hits.value[picked.value]) go(hits.value[picked.value].to);
  }
}
onMounted(() => addEventListener("keydown", onKey));
onBeforeUnmount(() => removeEventListener("keydown", onKey));
</script>

<template>
  <button class="search-open" @click="show"><span>Search the docs</span><kbd>⌘K</kbd></button>
  <!-- In the body: inside the top bar, its blur would hold the overlay to the bar. -->
  <Teleport to="body">
    <div class="search" :class="{ open }" @click.self="open = false">
      <div class="box">
        <input ref="input" v-model="query" type="search" placeholder="Search the docs" aria-label="Search">
        <ul v-if="hits.length || (query && sections)">
          <li v-if="!hits.length" class="empty">Nothing found.</li>
          <li v-for="(hit, i) in hits" :key="hit.to">
            <a :href="hit.to" :class="{ on: i === picked }" @click.prevent="go(hit.to)">{{ hit.title }}<small>{{ hit.page }}</small></a>
          </li>
        </ul>
      </div>
    </div>
  </Teleport>
</template>
