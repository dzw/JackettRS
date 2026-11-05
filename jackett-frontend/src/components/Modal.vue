<script setup>
import { onMounted, onBeforeUnmount } from "vue";

defineProps({
  title: { type: String, required: true },
  description: { type: String, default: "" },
});
const emit = defineEmits(["close"]);

function onKeydown(e) {
  if (e.key === "Escape") emit("close");
}

onMounted(() => document.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => document.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="modal" @click.self="emit('close')">
    <div class="modal-box">
      <div class="modal-head">
        <h2>{{ title }}</h2>
        <button type="button" class="modal-close" aria-label="关闭" @click="emit('close')">×</button>
      </div>
      <p v-if="description" class="modal-desc">{{ description }}</p>
      <slot />
    </div>
  </div>
</template>
