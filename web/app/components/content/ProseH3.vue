<script setup>
// A heading, and a # beside it to link to it.
defineProps({ id: { type: String, default: null } });
</script>

<template>
  <h3 :id="id"><slot /><a v-if="id" class="anchor" :href="`#${id}`" aria-hidden="true">#</a></h3>
</template>
