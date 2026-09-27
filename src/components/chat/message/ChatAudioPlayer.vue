<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Pause, Play } from 'lucide-vue-next'

const props = withDefaults(
  defineProps<{
    src: string | null
    variant?: 'user' | 'default'
  }>(),
  { variant: 'default' }
)

const audioRef = ref<HTMLAudioElement | null>(null)
const playing = ref(false)
const duration = ref(0)
const currentTime = ref(0)
const loading = ref(false)

const BAR_HEIGHTS = [3, 6, 4, 8, 5, 9, 6, 7, 4, 8, 5, 7, 6, 9, 4, 8, 5, 6, 7, 4, 8, 6, 5, 7, 4, 6, 8, 5]

const progress = computed(() =>
  duration.value > 0 ? Math.min(1, currentTime.value / duration.value) : 0
)

const bubbleClass = computed(() =>
  props.variant === 'user'
    ? 'bg-[hsl(var(--accent-muted))] border-accent/15'
    : 'bg-[hsl(var(--card-elevated))] border-border'
)

const playBtnClass = computed(() =>
  props.variant === 'user'
    ? 'bg-accent text-accent-foreground hover:bg-accent/90'
    : 'bg-foreground/10 text-foreground hover:bg-foreground/15'
)

function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00'
  const total = Math.floor(seconds)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}

const timeLabel = computed(() => {
  if (playing.value || currentTime.value > 0) {
    return formatTime(currentTime.value)
  }
  return formatTime(duration.value)
})

async function togglePlay() {
  const el = audioRef.value
  if (!el || !props.src) return
  if (playing.value) {
    el.pause()
    return
  }
  try {
    await el.play()
  } catch (e) {
    console.warn('audio play failed', e)
  }
}

function onTimeUpdate() {
  const el = audioRef.value
  if (!el) return
  currentTime.value = el.currentTime
}

function onLoadedMetadata() {
  const el = audioRef.value
  if (!el) return
  duration.value = el.duration
  loading.value = false
}

function onPlay() {
  playing.value = true
}

function onPause() {
  playing.value = false
}

function onEnded() {
  playing.value = false
  currentTime.value = 0
}

function onAudioError() {
  loading.value = false
  playing.value = false
  console.warn('audio element failed to load', props.src?.slice(0, 64))
}

function onSeek(e: MouseEvent) {
  const el = audioRef.value
  const target = e.currentTarget as HTMLElement
  if (!el || !duration.value) return
  const rect = target.getBoundingClientRect()
  const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width))
  el.currentTime = ratio * duration.value
}

watch(
  () => props.src,
  () => {
    playing.value = false
    currentTime.value = 0
    duration.value = 0
    loading.value = !!props.src
  },
  { immediate: true }
)

onBeforeUnmount(() => {
  const el = audioRef.value
  if (el) el.pause()
})
</script>

<template>
  <div
    class="inline-flex min-w-[220px] max-w-[280px] items-center gap-2.5 rounded-2xl border px-3 py-2.5"
    :class="bubbleClass"
  >
    <button
      type="button"
      class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full transition-colors disabled:opacity-40"
      :class="playBtnClass"
      :disabled="!src || loading"
      :aria-label="playing ? t('chat.pause') : t('chat.play')"
      @click="togglePlay"
    >
      <Pause v-if="playing" class="h-4 w-4" />
      <Play v-else class="h-4 w-4 translate-x-[1px]" />
    </button>

    <div class="min-w-0 flex-1 flex flex-col gap-1">
      <button
        type="button"
        class="flex h-7 w-full items-center gap-[2px] cursor-pointer"
        :disabled="!src || loading"
        :aria-label="t('chat.s_ee9310')"
        @click="onSeek"
      >
        <span
          v-for="(height, index) in BAR_HEIGHTS"
          :key="index"
          class="w-[3px] rounded-full transition-colors duration-150"
          :class="[
            index / BAR_HEIGHTS.length <= progress
              ? variant === 'user'
                ? 'bg-accent'
                : 'bg-foreground/70'
              : 'bg-foreground/20',
          ]"
          :style="{ height: `${height + 6}px` }"
        />
      </button>
      <span class="text-[10px] leading-none text-muted tabular-nums">
        {{ loading ? t('common.loading') : timeLabel }}
      </span>
    </div>

    <audio
      v-if="src"
      ref="audioRef"
      class="hidden"
      preload="metadata"
      :src="src"
      @timeupdate="onTimeUpdate"
      @loadedmetadata="onLoadedMetadata"
      @play="onPlay"
      @pause="onPause"
      @ended="onEnded"
      @error="onAudioError"
    />
  </div>
</template>
