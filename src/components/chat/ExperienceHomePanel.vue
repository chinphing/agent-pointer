<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
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
import { Search } from 'lucide-vue-next'
import ExperienceHomeCard from './ExperienceHomeCard.vue'

const FEATURED_TAB_ID = '__featured__'

const chat = useChatStore()
const settings = useSettingsStore()

const home = ref<ExperienceHomeResponse | null>(null)
const searchInput = ref('')
const searchQuery = ref('')
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
    <div class="relative">
      <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
      <input
        v-model="searchInput"
        type="search"
        placeholder="搜索经验…"
        class="w-full rounded-xl border border-border/80 bg-panel py-2.5 pl-9 pr-3 text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-accent/30"
        autocomplete="off"
      />
    </div>

    <template v-if="isSearching">
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
        <div
          class="-mx-1 flex gap-1 overflow-x-auto px-1 pb-1 [scrollbar-width:thin]"
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
