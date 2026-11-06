<script setup>
defineEmits(["menu"]);
const { version } = useRuntimeConfig().public;
</script>

<template>
  <header class="top">
    <button class="menu" aria-label="Pages" @click="$emit('menu')">☰</button>
    <NuxtLink class="brand" to="/">
      <img src="/favicon.svg" alt="">
      <span>Pulpo</span>
      <span class="version">v{{ version }}</span>
      <span class="label">Docs</span>
    </NuxtLink>
    <div class="spacer" />
    <DocsSearch />
    <a class="github" href="https://github.com/boardchow/pulpo">GitHub</a>
  </header>
</template>
