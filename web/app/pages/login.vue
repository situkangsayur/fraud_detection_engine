<script setup lang="ts">
import type { Me } from '#shared/types/api'
import { parseProblem, problemMessage } from '#shared/utils/problem'

definePageMeta({ layout: 'auth' })
const { t } = useI18n()
const route = useRoute()
const { fetch: refreshSession } = useUserSession()
const session = useSessionStore()
useHead({ title: () => t('login.title') })

const form = reactive({ email: '', password: '' })

function useDemo(a: { email: string, password: string }) {
  form.email = a.email
  form.password = a.password
}
const error = ref('')
const busy = ref(false)

async function submit() {
  error.value = ''
  busy.value = true
  try {
    const res = await $fetch<{ me: Me | null }>('/api/auth/login', { method: 'POST', body: form, headers: { 'x-requested-with': 'fraud-web' } })
    await refreshSession()
    session.set(res.me)
    let next = typeof route.query.next === 'string' && route.query.next.startsWith('/') ? route.query.next : '/'
    // a project link from an older session may point to a project this user cannot see (any more)
    const nextPid = next.match(/^\/p\/([^/?#]+)/)?.[1]
    if (nextPid && !res.me?.projects.some(p => p.id === nextPid)) next = '/projects'
    await navigateTo(next)
  }
  catch (err) {
    const e = err as { status?: number, data?: unknown }
    error.value = problemMessage(parseProblem(e.status ?? 0, e.data))
  }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="w-full max-w-5xl flex flex-col items-center gap-6 lg:flex-row lg:items-start lg:justify-center">
    <UCard class="w-full max-w-sm shrink-0">
      <template #header>
        <div class="flex items-center gap-2">
          <UIcon
            name="i-lucide-shield-check"
            class="size-7 text-primary"
          />
          <div>
            <h1 class="font-semibold text-lg">
              {{ t('app.name') }}
            </h1>
            <p class="text-xs text-muted">
              {{ t('login.subtitle') }}
            </p>
          </div>
        </div>
      </template>
      <form
        class="space-y-4"
        @submit.prevent="submit"
      >
        <UFormField
          :label="t('login.email')"
          name="email"
          required
        >
          <UInput
            v-model="form.email"
            type="email"
            autocomplete="username"
            class="w-full"
            autofocus
          />
        </UFormField>
        <UFormField
          :label="t('login.password')"
          name="password"
          required
        >
          <UInput
            v-model="form.password"
            type="password"
            autocomplete="current-password"
            class="w-full"
          />
        </UFormField>
        <UAlert
          v-if="error"
          color="error"
          variant="subtle"
          :title="t('login.failed')"
          :description="error"
          icon="i-lucide-circle-alert"
        />
        <UButton
          type="submit"
          block
          :loading="busy"
          :label="t('actions.login')"
        />
      </form>
      <template #footer>
        <div class="flex items-center justify-between">
          <p class="text-xs text-muted">
            {{ t('login.hint') }}
          </p>
          <div class="flex items-center gap-2">
            <LocaleSwitch />
            <UColorModeButton size="xs" />
          </div>
        </div>
      </template>
    </UCard>
    <DemoAccounts
      class="max-w-xl"
      @use="useDemo"
    />
  </div>
</template>
