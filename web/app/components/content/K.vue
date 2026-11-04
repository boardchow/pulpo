<script setup>
// :k[esc] in Markdown, or :k[⌃b ⌃b] for keys one after the other; :k{k="["}
// for a key Markdown would read as a bracket.
const props = defineProps({ k: { type: String, default: null } });
const slots = useSlots();
const keys = () => (props.k ?? slotText(slots.default?.())).trim().split(/\s+/);
</script>

<template>
  <span class="keys"><kbd v-for="(key, i) in keys()" :key="i">{{ key }}</kbd></span>
</template>
