<script setup lang="ts">
// Landing: go straight to the only project, else the project list (platform admins → tenants).
const session = useSessionStore()
await session.load()
if (session.isPlatformAdmin && !session.projects.length) await navigateTo('/admin/tenants', { replace: true })
else if (session.projects.length === 1) await navigateTo(`/p/${session.projects[0]!.id}`, { replace: true })
else await navigateTo('/projects', { replace: true })
</script>

<template>
  <div />
</template>
