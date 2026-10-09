<script setup lang="ts">
// Required attribution (AGPL-3.0 §13 + NOTICE §7(b)): shown on every page, links to the source code.
const config = useRuntimeConfig()
</script>

<template>
  <footer class="text-xs text-muted flex flex-wrap items-center justify-center gap-x-1.5 gap-y-1 py-3">
    <span>Fraud Detection Platform</span>
    <span aria-hidden="true">·</span>
    <span>© Hendri Karisma</span>
    <span aria-hidden="true">·</span>
    <NuxtLink
      to="/about"
      class="hover:text-primary underline-offset-2 hover:underline"
    >AGPL-3.0</NuxtLink>
    <span aria-hidden="true">·</span>
    <a
      :href="String(config.public.sourceUrl)"
      target="_blank"
      rel="noopener"
      class="hover:text-primary underline-offset-2 hover:underline"
    >Source code</a>
  </footer>
</template>
