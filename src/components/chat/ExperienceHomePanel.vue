<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import {
  listExperienceHome,
  searchExperiences,
  getExperienceDetail,
} from '../../lib/experiences'
import type {
  ExperienceHomeCategoryBlock,
  ExperienceHomeResponse,
  ExperienceListItem,
} from '../../types/experience'
import { useChatStore } from '../../stores/chat'
import { Search, X } from 'lucide-vue-next'
import ExperienceHomeCard from './ExperienceHomeCard.vue'
import { usePlatformAuthStore } from '../../stores/platformAuth'

const FEATURED_TAB_ID = '__featured__'

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()

const home = ref<ExperienceHomeResponse | null>(null)
const searchInput = ref('')
const searchQuery = ref('')
const searchExpanded = ref(false)
const searchInputEl = ref<HTMLInputElement | null>(null)
const searchResults = ref<ExperienceListItem[]>([])
const searchLoading = ref(false)
const activeTabId = ref<string | null>(null)
const loadingSlug = ref<string | null>(null)
const loadErr = ref<string | null>(null)

const CARD_TONES = [
  'experience-card--sky',
  'experience-card--rose',
  'experience-card--sand',
] as const

const isSearching = computed(() => searchQuery.value.length > 0)
const featuredItems = computed(() => home.value?.featured ?? [])
const categoryBlocks = computed(() => home.value?.categories ?? [])

const homeTabs = computed((): ExperienceHomeCategoryBlock[] => {
  const tabs: ExperienceHomeCategoryBlock[] = []
  if (featuredItems.value.length > 0) {
    tabs.push({
      id: FEATURED_TAB_ID,
      name_zh: '热门',
      items: featuredItems.value,
    })
  }
  tabs.push(...categoryBlocks.value)
  return tabs
})

const activeTab = computed((): ExperienceHomeCategoryBlock | null => {
  const tabs = homeTabs.value
  if (tabs.length === 0) return null
  const id = activeTabId.value
  return tabs.find((tab) => tab.id === id) ?? tabs[0] ?? null
})

let searchTimer: ReturnType<typeof setTimeout> | undefined

watch(searchInput, (v) => {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    searchQuery.value = v.trim()
  }, 300)
})

watch(searchQuery, async (q) => {
  if (!q) {
    searchResults.value = []
    return
  }
  searchLoading.value = true
  try {
    searchResults.value = await searchExperiences(q, 20)
  } catch (e) {
    console.warn('experience search failed', e)
    searchResults.value = []
  } finally {
    searchLoading.value = false
  }
})

onMounted(async () => {
  if (platformAuth.isStandalone) return
  try {
    const data = await listExperienceHome()
    home.value = data
    if (data.featured.length > 0) {
      activeTabId.value = FEATURED_TAB_ID
    } else if (data.categories.length > 0) {
      activeTabId.value = data.categories[0].id
    }
  } catch (e) {
    console.warn('experience home load failed', e)
    loadErr.value = '经验加载失败'
  }
})

function cardTone(index: number): string {
  return CARD_TONES[index % CARD_TONES.length]
}

function resolveAgentId(raw: string | undefined | null): string {
  const id = (raw ?? 'general').trim()
  if (id === 'coder' || id === 'computer') return id
  return 'general'
}

async function openSearch() {
  searchExpanded.value = true
  await nextTick()
  searchInputEl.value?.focus()
}

function closeSearch() {
  searchExpanded.value = false
  searchInput.value = ''
  searchQuery.value = ''
}

async function onSelect(item: ExperienceListItem) {
  if (loadingSlug.value) return
  loadingSlug.value = item.slug
  try {
    const detail = await getExperienceDetail(item.slug)
    const agentId = resolveAgentId(detail.agent_id ?? item.agent_id)
    if (!chat.current) chat.newConversation()
    chat.setConversationAgent(agentId, 'single')
    const prompt = detail.prompt_text?.trim()
    if (prompt) {
      chat.prefillComposer(prompt)
    }
  } catch (e) {
    console.warn('experience detail load failed', e)
    chat.showUiToast('经验加载失败，请稍后重试', 'warning')
  } finally {
    loadingSlug.value = null
  }
}
</script>

<template>
  <section class="w-full space-y-4">
    <template v-if="!home && !loadErr">
      <p class="text-center text-xs text-muted">加载中…</p>
    </template>

    <template v-else-if="loadErr">
      <p class="text-center text-sm text-muted">{{ loadErr }}</p>
    </template>

    <template v-else>
      <div v-if="homeTabs.length > 0" class="space-y-3">
        <div class="flex min-w-0 items-center gap-1.5">
          <div
            class="relative flex h-7 shrink-0 items-center overflow-hidden rounded-full bg-accent/10 transition-[width] duration-200 ease-out"
            :class="searchExpanded ? 'w-44 sm:w-52' : ''"
          >
            <button
              v-if="!searchExpanded"
              type="button"
              class="inline-flex h-7 w-7 shrink-0 items-center justify-center text-muted transition-colors hover:text-foreground"
              aria-label="搜索经验"
              @click="openSearch"
            >
              <Search class="h-3.5 w-3.5" />
            </button>
            <template v-else>
              <Search class="pointer-events-none absolute left-2.5 h-3.5 w-3.5 text-muted" />
              <input
                ref="searchInputEl"
                v-model="searchInput"
                type="search"
                placeholder="搜索…"
                class="h-7 w-full min-w-0 rounded-full border-0 bg-transparent py-0 pl-8 pr-8 text-xs leading-none outline-none focus-visible:ring-2 focus-visible:ring-accent/30"
                autocomplete="off"
                @keydown.esc="closeSearch"
              />
              <button
                type="button"
                class="absolute right-1.5 inline-flex h-5 w-5 items-center justify-center rounded-full text-muted hover:bg-hover hover:text-foreground"
                aria-label="关闭搜索"
                @click="closeSearch"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </template>
          </div>

          <div
            v-if="!isSearching"
            class="-mx-0.5 flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto px-0.5 [scrollbar-width:thin]"
            role="tablist"
          >
            <button
              v-for="tab in homeTabs"
              :key="tab.id"
              type="button"
              role="tab"
              class="inline-flex h-7 shrink-0 items-center rounded-full px-3 text-xs font-medium leading-none transition-colors"
              :class="
                activeTab?.id === tab.id
                  ? 'bg-accent text-white'
                  : 'bg-accent/10 text-muted hover:text-foreground'
              "
              :aria-selected="activeTab?.id === tab.id"
              @click="activeTabId = tab.id"
            >
              {{ tab.name_zh }}
            </button>
          </div>
        </div>

        <div v-if="isSearching" role="search">
          <p v-if="searchLoading" class="text-center text-xs text-muted">搜索中…</p>
          <p v-else-if="searchResults.length === 0" class="text-center text-sm text-muted">
            未找到相关经验
          </p>
          <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <ExperienceHomeCard
              v-for="(item, index) in searchResults"
              :key="item.id"
              :item="item"
              :tone="cardTone(index)"
              :disabled="loadingSlug === item.slug"
              @select="onSelect"
            />
          </div>
        </div>

        <div v-else-if="activeTab" role="tabpanel" class="min-h-[8rem]">
          <div
            v-if="activeTab.items.length === 0"
            class="flex min-h-[8rem] items-center justify-center rounded-2xl border border-dashed border-border/70 px-4 py-8 text-sm text-muted"
          >
            板块建设中，敬请期待...
          </div>
          <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <ExperienceHomeCard
              v-for="(item, index) in activeTab.items"
              :key="item.id"
              :item="item"
              :tone="cardTone(index)"
              :disabled="loadingSlug === item.slug"
              @select="onSelect"
            />
          </div>
        </div>
      </div>
    </template>
  </section>
</template>
