<script setup>
defineProps({ id: { type: String, default: null } });
</script>

<template>
  <h2 :id="id"><slot /><a v-if="id" class="anchor" :href="`#${id}`" aria-hidden="true">#</a></h2>
</template>
