<script setup>
// :kp[n] in Markdown: the prefix, then n; :kp{k="["} for a bracket. (Not
// :p, which is a paragraph.)
const props = defineProps({ k: { type: String, default: null } });
const slots = useSlots();
const keys = () => (props.k ?? slotText(slots.default?.())).trim().split(/\s+/).filter(Boolean);
</script>

<template>
  <span class="keys"><kbd class="prefix">⌘p</kbd><kbd v-for="(key, i) in keys()" :key="i">{{ key }}</kbd></span>
</template>
