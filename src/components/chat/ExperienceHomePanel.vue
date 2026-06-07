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

const activeCategory = computed((): ExperienceHomeCategoryBlock | null => {
  const blocks = categoryBlocks.value
  if (blocks.length === 0) return null
  const id = activeTabId.value
  return blocks.find((b) => b.id === id) ?? blocks[0] ?? null
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
    if (data.categories.length > 0) {
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

function displayTitle(title: string): string {
  return title.replace(/^【[^】]+】\s*/, '').trim() || title
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
        <button
          v-for="(item, index) in searchResults"
          :key="item.id"
          type="button"
          class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
          :class="cardTone(index)"
          :disabled="loadingSlug === item.slug"
          @click="onSelect(item)"
        >
          <div
            v-if="item.cover_image_url"
            class="mb-2.5 aspect-video overflow-hidden rounded-lg bg-black/5"
          >
            <img
              :src="item.cover_image_url"
              alt=""
              class="h-full w-full object-cover"
              loading="lazy"
            />
          </div>
          <div class="experience-card__title">{{ displayTitle(item.title) }}</div>
          <div class="experience-card__body">
            <p class="line-clamp-3">{{ item.excerpt || ' ' }}</p>
          </div>
        </button>
      </div>
    </template>

    <template v-else-if="!home && !loadErr">
      <p class="text-center text-xs text-muted">加载中…</p>
    </template>

    <template v-else>
      <div v-if="featuredItems.length > 0" class="space-y-2">
        <h2 class="text-left text-xs font-semibold tracking-wide text-muted">热门</h2>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <button
            v-for="(item, index) in featuredItems"
            :key="item.id"
            type="button"
            class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
            :class="cardTone(index)"
            :disabled="loadingSlug === item.slug"
            @click="onSelect(item)"
          >
            <div
              v-if="item.cover_image_url"
              class="mb-2.5 aspect-video overflow-hidden rounded-lg bg-black/5"
            >
              <img
                :src="item.cover_image_url"
                alt=""
                class="h-full w-full object-cover"
                loading="lazy"
              />
            </div>
            <div class="experience-card__title">{{ displayTitle(item.title) }}</div>
            <div class="experience-card__body">
              <p class="line-clamp-3">{{ item.excerpt || ' ' }}</p>
            </div>
          </button>
        </div>
      </div>

      <div v-if="categoryBlocks.length > 0" class="space-y-3">
        <div
          class="-mx-1 flex gap-1 overflow-x-auto px-1 pb-1 [scrollbar-width:thin]"
          role="tablist"
        >
          <button
            v-for="cat in categoryBlocks"
            :key="cat.id"
            type="button"
            role="tab"
            class="shrink-0 rounded-full px-3 py-1.5 text-xs font-medium transition-colors"
            :class="
              activeCategory?.id === cat.id
                ? 'bg-foreground text-background'
                : 'bg-accent/10 text-muted hover:text-foreground'
            "
            :aria-selected="activeCategory?.id === cat.id"
            @click="activeTabId = cat.id"
          >
            {{ cat.name_zh }}
          </button>
        </div>

        <div v-if="activeCategory" role="tabpanel" class="min-h-[8rem]">
          <div
            v-if="activeCategory.items.length === 0"
            class="flex min-h-[8rem] items-center justify-center rounded-2xl border border-dashed border-border/70 px-4 py-8 text-sm text-muted"
          >
            板块建设中，敬请期待...
          </div>
          <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <button
              v-for="(item, index) in activeCategory.items"
              :key="item.id"
              type="button"
              class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
              :class="cardTone(index)"
              :disabled="loadingSlug === item.slug"
              @click="onSelect(item)"
            >
              <div
                v-if="item.cover_image_url"
                class="mb-2.5 aspect-video overflow-hidden rounded-lg bg-black/5"
              >
                <img
                  :src="item.cover_image_url"
                  alt=""
                  class="h-full w-full object-cover"
                  loading="lazy"
                />
              </div>
              <div class="experience-card__title">{{ displayTitle(item.title) }}</div>
              <div class="experience-card__body">
                <p class="line-clamp-3">{{ item.excerpt || ' ' }}</p>
              </div>
            </button>
          </div>
        </div>
      </div>

      <p v-else-if="loadErr" class="text-center text-sm text-muted">{{ loadErr }}</p>
    </template>
  </section>
</template>

<style scoped>
.experience-card {
  @apply flex h-full flex-col rounded-2xl px-4 py-5;
}

.experience-card__title {
  @apply shrink-0 text-sm font-semibold leading-[1.375] text-foreground line-clamp-2;
  height: 2.75rem;
}

.experience-card__body {
  @apply mt-2.5 min-h-[3.75rem] text-xs leading-relaxed text-muted;
}

.experience-card--sky {
  background: #eef4ff;
}
.experience-card--rose {
  background: #fff0f3;
}
.experience-card--sand {
  background: #fff8eb;
}

.experience-card:hover {
  filter: brightness(0.98);
  transform: translateY(-1px);
}

html.dark .experience-card--sky {
  background: hsl(220 60% 18% / 0.55);
}
html.dark .experience-card--rose {
  background: hsl(350 45% 18% / 0.55);
}
html.dark .experience-card--sand {
  background: hsl(38 45% 16% / 0.55);
}

html.dark .experience-card:hover {
  filter: brightness(1.08);
}
</style>
