<script setup>
defineProps({
  version: { type: String, default: "" },
  healthOk: { type: Boolean, default: null },
});

const host = location.host;
</script>

<template>
  <header class="app-header">
    <h1>Jackett <span style="color: var(--dim); font-weight: 400">(Rust)</span></h1>
    <span class="badge">{{ version || "version" }}</span>
    <span class="badge">
      <span class="dot" :class="healthOk === true ? 'on' : healthOk === false ? 'off' : ''"></span>
      <span>{{ healthOk === null ? "checking…" : healthOk ? "running" : "unhealthy" }}</span>
    </span>
    <span class="badge">{{ host }}</span>
  </header>
</template>
