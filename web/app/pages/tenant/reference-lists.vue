<script setup lang="ts">
const { t } = useI18n()
const session = useSessionStore()
const route = useRoute()
// Tenant admins manage their own tenant; platform admins arrive from /admin/tenants with ?tid=
const tid = computed(() => (route.query.tid as string | undefined) ?? session.tenant?.id)
useHead({ title: () => t('nav.tenantLists') })
</script>

<template>
  <PagePanel
    :title="t('nav.tenantLists')"
    :description="t('lists.tenantDescription')"
  >
    <ReferenceListsView
      v-if="tid"
      :base-path="`/tenants/${tid}/reference-lists`"
      :can-edit="session.isTenantAdmin || session.isPlatformAdmin"
    />
    <UEmpty
      v-else
      icon="i-lucide-building-2"
      :title="t('tenants.noTenant')"
    />
  </PagePanel>
</template>
