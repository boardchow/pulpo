<script setup>
// The docs' top bar: the pulpo, its name and version as on the landing
// page, the search, GitHub.
defineEmits(["menu"]);
// Cargo.toml's version, shown by the name.
const VERSION = "0.1.0";
</script>

<template>
  <header class="top">
    <button class="menu" aria-label="Pages" @click="$emit('menu')">☰</button>
    <NuxtLink class="brand" to="/">
      <img src="/favicon.svg" alt="">
      <span>Pulpo</span>
      <span class="version">v{{ VERSION }}</span>
      <span class="label">Docs</span>
    </NuxtLink>
    <div class="spacer" />
    <DocsSearch />
    <a class="github" href="https://github.com/boardchow/pulpo">GitHub</a>
  </header>
</template>
