<script setup>
// Code, with a button that copies it.
const props = defineProps({
  code: { type: String, default: "" },
  language: { type: String, default: null },
  filename: { type: String, default: null },
  highlights: { type: Array, default: () => [] },
  meta: { type: String, default: null },
  class: { type: String, default: null },
  style: { type: [String, Object], default: null },
});
const label = ref("Copy");
async function copy() {
  await navigator.clipboard.writeText(props.code.trim());
  label.value = "Copied";
  setTimeout(() => (label.value = "Copy"), 1500);
}
</script>

<template>
  <pre :class="props.class" :style="style"><slot /><button class="copy" type="button" @click="copy">{{ label }}</button></pre>
</template>
