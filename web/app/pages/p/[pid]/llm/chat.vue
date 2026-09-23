<script setup lang="ts">
// LLM assistant chat (SSE streaming). The assistant uses read-only tools (rules, stats, drift, graph, regulations)
// and can only create *pending* proposals — it never changes rules itself.
import type { ChatMessage, Citation, Conversation, Page, ToolCallSummary } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { pid, base, apiBase } = useProject()
useHead({ title: () => t('nav.llmChat') })

const { data: conversations, refresh: refreshConvs } = await useAsyncData<Conversation[]>(`convs-${pid.value}`, () => api.get<Page<Conversation>>(`${apiBase.value}/llm/conversations`, { query: { page_size: 50 }, silent: true }).then(p => p.items).catch(() => []), { default: () => [] as Conversation[] })
const conversationId = ref<string | null>((route.query.c as string) ?? null)
const messages = ref<ChatMessage[]>([])
const input = ref('')
const streaming = ref(false)
let abort: AbortController | null = null
const scroller = ref<HTMLElement>()

async function openConversation(id: string | null) {
  conversationId.value = id
  messages.value = id ? (await api.get<Conversation>(`${apiBase.value}/llm/conversations/${id}`)).messages ?? [] : []
  scrollDown()
}
if (conversationId.value) await openConversation(conversationId.value)

function scrollDown() {
  nextTick(() => scroller.value?.scrollTo({ top: scroller.value.scrollHeight, behavior: 'smooth' }))
}

const SUGGESTIONS = computed(() => [t('llm.suggest.situation'), t('llm.suggest.relevance'), t('llm.suggest.regulation'), t('llm.suggest.newRules')])

async function send(text?: string) {
  const message = (text ?? input.value).trim()
  if (!message || streaming.value) return
  input.value = ''
  messages.value.push({ role: 'user', content: message })
  const reply = reactive<ChatMessage>({ role: 'assistant', content: '', tool_calls: [], citations: [] })
  messages.value.push(reply)
  streaming.value = true
  abort = new AbortController()
  scrollDown()
  try {
    await api.stream(`${apiBase.value}/llm/chat/stream`, { conversation_id: conversationId.value ?? undefined, message }, (event, data) => {
      if (event === 'token') reply.content += (data as { content?: string, text?: string }).content ?? (data as { text?: string }).text ?? ''
      else if (event === 'tool') reply.tool_calls!.push(data as ToolCallSummary)
      else if (event === 'citation') reply.citations!.push(data as Citation)
      else if (event === 'done' && !reply.content && (data as { answer?: string }).answer) {
        reply.content = (data as { answer: string }).answer
        conversationId.value = (data as { conversation_id?: string }).conversation_id ?? conversationId.value
      }
      else if (event === 'conversation' || event === 'meta' || event === 'done') {
        const cid = (data as { conversation_id?: string }).conversation_id
        if (cid && cid !== conversationId.value) conversationId.value = cid
      }
      else if (event === 'error') reply.content += `\n\n⚠️ ${(data as { detail?: string }).detail ?? t('errors.generic')}`
      scrollDown()
    }, abort.signal)
  }
  catch (err) {
    if ((err as Error).name !== 'AbortError') reply.content ||= t('errors.generic')
  }
  finally {
    streaming.value = false
    abort = null
    refreshConvs()
  }
}
function stop() { abort?.abort() }
</script>

<template>
  <PagePanel :title="t('nav.llmChat')">
    <template #actions>
      <UButton
        icon="i-lucide-plus"
        :label="t('llm.newChat')"
        color="neutral"
        variant="outline"
        @click="openConversation(null)"
      />
    </template>
    <div class="grid lg:grid-cols-[16rem_1fr] gap-4 h-[calc(100vh-9rem)]">
      <UCard
        class="hidden lg:block overflow-y-auto"
        :ui="{ body: 'p-2 sm:p-2' }"
      >
        <p class="text-xs text-muted uppercase px-2 py-1">
          {{ t('llm.conversations') }}
        </p>
        <UButton
          v-for="c in conversations"
          :key="c.id"
          block
          :color="c.id === conversationId ? 'primary' : 'neutral'"
          :variant="c.id === conversationId ? 'soft' : 'ghost'"
          class="justify-start truncate"
          :label="c.title ?? shortId(c.id)"
          @click="openConversation(c.id)"
        />
        <p
          v-if="!conversations.length"
          class="text-xs text-muted px-2"
        >
          {{ t('llm.noConversations') }}
        </p>
      </UCard>

      <div class="flex flex-col min-h-0">
        <div
          ref="scroller"
          class="flex-1 overflow-y-auto space-y-4 pr-1"
          aria-live="polite"
        >
          <div
            v-if="!messages.length"
            class="max-w-2xl mx-auto text-center pt-10 space-y-4"
          >
            <UIcon
              name="i-lucide-sparkles"
              class="size-10 text-primary"
            />
            <h2 class="text-lg font-semibold">
              {{ t('llm.welcomeTitle') }}
            </h2>
            <p class="text-sm text-muted">
              {{ t('llm.welcomeBody') }}
            </p>
            <div class="grid sm:grid-cols-2 gap-2">
              <UButton
                v-for="s in SUGGESTIONS"
                :key="s"
                color="neutral"
                variant="outline"
                class="text-left whitespace-normal h-auto py-2"
                :label="s"
                @click="send(s)"
              />
            </div>
          </div>
          <div
            v-for="(m, i) in messages"
            :key="i"
            class="flex"
            :class="m.role === 'user' ? 'justify-end' : 'justify-start'"
          >
            <div
              class="max-w-3xl rounded-lg px-4 py-3"
              :class="m.role === 'user' ? 'bg-primary text-inverted' : 'bg-elevated'"
            >
              <p
                v-if="m.role === 'user'"
                class="text-sm whitespace-pre-wrap"
              >
                {{ m.content }}
              </p>
              <template v-else>
                <div
                  v-if="m.tool_calls?.length"
                  class="flex flex-wrap gap-1 mb-2"
                >
                  <UTooltip
                    v-for="(tc, j) in m.tool_calls"
                    :key="j"
                    :text="tc.result_summary ?? JSON.stringify(tc.args)"
                  >
                    <UBadge
                      color="neutral"
                      variant="outline"
                      size="xs"
                      icon="i-lucide-wrench"
                      class="fp-mono"
                    >
                      {{ tc.name }}
                    </UBadge>
                  </UTooltip>
                </div>
                <MarkdownView :source="m.content || '…'" />
                <div
                  v-if="m.citations?.length"
                  class="mt-3 border-t border-default pt-2 space-y-1"
                >
                  <p class="text-xs text-muted uppercase">
                    {{ t('llm.citations') }}
                  </p>
                  <p
                    v-for="(c, j) in m.citations"
                    :key="j"
                    class="text-xs"
                  >
                    <UIcon
                      name="i-lucide-scale"
                      class="align-middle"
                    /> <b>{{ c.code }} · {{ c.section }}</b> — <i class="text-muted">{{ c.excerpt }}</i>
                  </p>
                </div>
              </template>
            </div>
          </div>
        </div>
        <form
          class="mt-3 flex gap-2 items-end"
          @submit.prevent="send()"
        >
          <UTextarea
            v-model="input"
            :rows="2"
            autoresize
            :maxrows="8"
            class="flex-1"
            :placeholder="t('llm.placeholder')"
            :disabled="streaming"
            @keydown.enter.exact.prevent="send()"
          />
          <UButton
            v-if="streaming"
            icon="i-lucide-square"
            color="neutral"
            :label="t('llm.stop')"
            @click="stop"
          />
          <UButton
            v-else
            type="submit"
            icon="i-lucide-send"
            :label="t('actions.send')"
            :disabled="!input.trim()"
          />
        </form>
        <p class="text-xs text-muted mt-1">
          {{ t('llm.disclaimer') }} <NuxtLink
            :to="`${base}/proposals`"
            class="text-primary"
          >{{ t('nav.proposals') }}</NuxtLink>
        </p>
      </div>
    </div>
  </PagePanel>
</template>
