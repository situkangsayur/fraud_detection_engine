<script setup lang="ts">
// Sign-in hints for demo / experiment installs, grouped by tenant. Source: NUXT_PUBLIC_DEMO_ACCOUNTS
// ("Tenant|email|password|role;…", or the older "email|password|role;…" which lands in one group).
export interface DemoAccount { tenant: string, email: string, password: string, role: string }

const emit = defineEmits<{ use: [account: DemoAccount] }>()
const { t } = useI18n()
const toast = useToast()

const groups = computed(() => {
  const rows = String(useRuntimeConfig().public.demoAccounts || '')
    .split(';')
    .map(r => r.split('|').map(x => x.trim()))
    .filter(r => r.length >= 2 && r.some(Boolean))
  const accounts: DemoAccount[] = rows.map(r => (r.length >= 4
    ? { tenant: r[0]!, email: r[1]!, password: r[2]!, role: r[3]! }
    : { tenant: 'Demo', email: r[0]!, password: r[1]!, role: r[2] ?? '' }))
    .filter(a => a.email && a.password)
  const byTenant = new Map<string, DemoAccount[]>()
  for (const a of accounts) byTenant.set(a.tenant, [...(byTenant.get(a.tenant) ?? []), a])
  return [...byTenant].map(([tenant, items], i) => ({ label: tenant, value: String(i), accounts: items }))
})

const open = ref<string[]>(['0'])

const roleColor = (role: string) => {
  const r = role.toLowerCase()
  if (r.includes('platform')) return 'error' as const
  if (r.includes('tenant') || r.includes('admin')) return 'warning' as const
  if (r.includes('approver')) return 'success' as const
  if (r.includes('analyst')) return 'info' as const
  return 'neutral' as const
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    toast.add({ title: t('login.copied'), icon: 'i-lucide-check', duration: 1500 })
  }
  catch { /* clipboard blocked: nothing to do */ }
}
</script>

<template>
  <UCard
    v-if="groups.length"
    class="w-full"
    :ui="{ body: 'p-0 sm:p-0' }"
  >
    <template #header>
      <div class="flex items-start gap-3">
        <UIcon
          name="i-lucide-users-round"
          class="size-5 text-primary mt-0.5 shrink-0"
        />
        <div>
          <h2 class="font-semibold">
            {{ t('login.demoTitle') }}
          </h2>
          <p class="text-xs text-muted">
            {{ t('login.demoHint') }}
          </p>
        </div>
      </div>
    </template>

    <UAccordion
      v-model="open"
      type="multiple"
      :items="groups"
      :ui="{ trigger: 'px-4 py-2.5 min-w-0 gap-2', label: 'min-w-0 flex-1', content: 'px-2 pb-2' }"
    >
      <template #default="{ item }">
        <span class="flex items-center gap-2 min-w-0 flex-1">
          <UIcon
            name="i-lucide-building-2"
            class="size-4 text-muted shrink-0"
          />
          <span class="truncate font-medium text-sm">{{ item.label }}</span>
          <UBadge
            :label="String(item.accounts.length)"
            size="sm"
            variant="subtle"
            color="neutral"
          />
        </span>
      </template>

      <template #content="{ item }">
        <ul class="divide-y divide-default">
          <li
            v-for="a in item.accounts"
            :key="a.email"
            class="flex flex-col gap-1 px-2 py-2 rounded-md hover:bg-elevated/60 sm:flex-row sm:items-center sm:gap-3"
          >
            <span class="flex items-center gap-2 min-w-0 sm:flex-1">
              <UBadge
                :label="a.role || '—'"
                :color="roleColor(a.role)"
                variant="soft"
                size="sm"
                class="w-24 justify-center shrink-0"
              />
              <span
                class="font-mono text-xs truncate min-w-0"
                :title="a.email"
              >{{ a.email }}</span>
            </span>
            <span class="flex items-center gap-1 justify-end">
              <span class="font-mono text-xs text-muted mr-1">{{ a.password }}</span>
              <UButton
                size="xs"
                variant="ghost"
                color="neutral"
                icon="i-lucide-copy"
                :aria-label="t('login.copy')"
                @click="copy(a.email)"
              />
              <UButton
                size="xs"
                variant="soft"
                icon="i-lucide-log-in"
                :label="t('login.useAccount')"
                @click="emit('use', a)"
              />
            </span>
          </li>
        </ul>
      </template>
    </UAccordion>
  </UCard>
</template>
