<script setup>
defineProps({ title: { type: String, required: true }, to: { type: String, required: true } });
</script>

<template>
  <NuxtLink :to="to"><b>{{ title }}</b><span><slot mdc-unwrap="p" /></span></NuxtLink>
</template>
