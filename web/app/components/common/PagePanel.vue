<script setup lang="ts">
// Standard page frame: navbar with title/actions + scrollable body.
defineProps<{ title: string, description?: string, id?: string }>()
</script>

<template>
  <UDashboardPanel :id="id ?? title">
    <template #header>
      <UDashboardNavbar :title="title">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>
        <template #right>
          <slot name="actions" />
        </template>
      </UDashboardNavbar>
      <UDashboardToolbar v-if="$slots.toolbar">
        <slot name="toolbar" />
      </UDashboardToolbar>
    </template>
    <template #body>
      <p
        v-if="description"
        class="text-sm text-muted -mt-2 mb-2"
      >
        {{ description }}
      </p>
      <slot />
      <AppFooter class="mt-auto pt-6" />
    </template>
  </UDashboardPanel>
</template>
