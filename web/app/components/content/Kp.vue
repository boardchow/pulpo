<script setup>
// :kp, not :p (a paragraph); :kp{k="["} for a bracket.
const props = defineProps({ k: { type: String, default: null } });
const slots = useSlots();
const keys = () => (props.k ?? slotText(slots.default?.())).trim().split(/\s+/).filter(Boolean);
</script>

<template>
  <span class="keys"><kbd class="prefix">⌃b</kbd><kbd v-for="(key, i) in keys()" :key="i">{{ key }}</kbd></span>
</template>
