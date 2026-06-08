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
import { useSettingsStore } from '../../stores/settings'
import { Search, X } from 'lucide-vue-next'
import ExperienceHomeCard from './ExperienceHomeCard.vue'

const FEATURED_TAB_ID = '__featured__'

const chat = useChatStore()
const settings = useSettingsStore()

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
    await settings.saveAgentPreferences({ agentMode: 'single', leadAgentId: agentId })
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
  <section class="w-full space-y-5">
    <template v-if="isSearching">
      <div class="flex items-center justify-end gap-2">
        <div class="relative flex items-center">
          <Search class="pointer-events-none absolute left-2.5 h-3.5 w-3.5 text-muted" />
          <input
            ref="searchInputEl"
            v-model="searchInput"
            type="search"
            placeholder="搜索经验…"
            class="w-44 rounded-full border border-border/80 bg-panel py-1.5 pl-8 pr-8 text-xs outline-none focus-visible:ring-2 focus-visible:ring-accent/30 sm:w-52"
            autocomplete="off"
          />
          <button
            type="button"
            class="absolute right-1.5 rounded-full p-0.5 text-muted hover:bg-hover hover:text-foreground"
            aria-label="关闭搜索"
            @click="closeSearch"
          >
            <X class="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

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
    </template>

    <template v-else-if="!home && !loadErr">
      <p class="text-center text-xs text-muted">加载中…</p>
    </template>

    <template v-else>
      <div v-if="homeTabs.length > 0" class="space-y-3">
        <div class="flex items-center gap-2">
          <div
            class="-mx-1 flex min-w-0 flex-1 gap-1 overflow-x-auto px-1 pb-1 [scrollbar-width:thin]"
            role="tablist"
          >
            <button
              v-for="tab in homeTabs"
              :key="tab.id"
              type="button"
              role="tab"
              class="shrink-0 rounded-full px-3 py-1.5 text-xs font-medium transition-colors"
              :class="
                activeTab?.id === tab.id
                  ? 'bg-foreground text-background'
                  : 'bg-accent/10 text-muted hover:text-foreground'
              "
              :aria-selected="activeTab?.id === tab.id"
              @click="activeTabId = tab.id"
            >
              {{ tab.name_zh }}
            </button>
          </div>

          <div class="shrink-0 pb-1">
            <button
              v-if="!searchExpanded"
              type="button"
              class="flex h-7 w-7 items-center justify-center rounded-full text-muted transition-colors hover:bg-hover hover:text-foreground"
              aria-label="搜索经验"
              @click="openSearch"
            >
              <Search class="h-4 w-4" />
            </button>
            <div v-else class="relative flex items-center">
              <Search class="pointer-events-none absolute left-2.5 h-3.5 w-3.5 text-muted" />
              <input
                ref="searchInputEl"
                v-model="searchInput"
                type="search"
                placeholder="搜索…"
                class="w-32 rounded-full border border-border/80 bg-panel py-1.5 pl-8 pr-8 text-xs outline-none focus-visible:ring-2 focus-visible:ring-accent/30 sm:w-40"
                autocomplete="off"
                @keydown.esc="closeSearch"
              />
              <button
                type="button"
                class="absolute right-1.5 rounded-full p-0.5 text-muted hover:bg-hover hover:text-foreground"
                aria-label="关闭搜索"
                @click="closeSearch"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>

        <div v-if="activeTab" role="tabpanel" class="min-h-[8rem]">
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

      <p v-else-if="loadErr" class="text-center text-sm text-muted">{{ loadErr }}</p>
    </template>
  </section>
</template>
