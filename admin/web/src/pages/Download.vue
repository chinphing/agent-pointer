<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api } from '../api'

const { t, tm } = useI18n()

/** 后端产物类型 → 平台。只认**安装包**：更新包是给客户端 updater 用的，装不上。 */
const KIND_TO_PLATFORM: Record<string, string> = {
  macos_dmg: 'macos',
  windows_msi: 'windows',
  windows_nsis: 'windows',
  linux_deb: 'linux',
  linux_appimage: 'linux',
}

/** 官网的排列顺序：Windows / macOS / Linux。 */
const PLATFORM_ORDER = ['windows', 'macos', 'linux']

interface Artifact {
  id: string
  kind: string
  filename: string
  size: number
  downloadUrl: string
}

interface ReleaseItem {
  version: string
  notes: string
  publishedAt: number | null
  artifacts: Artifact[]
}

const loading = ref(true)
const error = ref('')
const items = ref<ReleaseItem[]>([])

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** 只取最新一个已发布版本 —— 官网也是按版本渲染平台卡。 */
const latest = computed<ReleaseItem | null>(() => items.value[0] ?? null)

/** 产物按平台归组。 */
const byPlatform = computed<Record<string, Artifact[]>>(() => {
  const out: Record<string, Artifact[]> = {}
  for (const a of latest.value?.artifacts ?? []) {
    const platform = KIND_TO_PLATFORM[a.kind]
    if (!platform) continue
    ;(out[platform] ??= []).push(a)
  }
  return out
})

/** 当前浏览器的平台，用来把对应的卡片高亮成「本机」。 */
const localPlatform = ref('')

function detectPlatform(): string {
  const ua = (navigator.userAgent || '').toLowerCase()
  if (ua.includes('windows')) return 'windows'
  if (ua.includes('mac')) return 'macos'
  if (ua.includes('linux')) return 'linux'
  return ''
}

const platforms = computed(() =>
  PLATFORM_ORDER.map((id) => ({
    id,
    name: t(`download.${id}`),
    minVersion: t(`download.req.${id}`),
    steps: (tm(`download.steps.${id}`) as string[]) ?? [],
    artifacts: byPlatform.value[id] ?? [],
  }))
)

/** 三列时把本机平台放中间，视觉焦点更自然（与官网一致）。 */
const orderedPlatforms = computed(() => {
  const list = platforms.value
  if (list.length !== 3 || !localPlatform.value) return list
  const active = list.find((p) => p.id === localPlatform.value)
  const others = list.filter((p) => p.id !== localPlatform.value)
  if (!active || others.length !== 2) return list
  return [others[0], active, others[1]]
})

function sizeMb(bytes: number): string {
  if (!bytes) return ''
  // 测试或占位产物可能很小，直接给真实单位，别四舍五入成 0.0 KB。
  if (bytes < 1024) return `${bytes} B`
  const mb = bytes / (1024 * 1024)
  if (mb < 1) return `${(bytes / 1024).toFixed(1)} KB`
  return `${mb.toFixed(1)} MB`
}

function when(ts: number | null): string {
  if (!ts) return ''
  return new Date(ts * 1000).toLocaleDateString()
}

onMounted(async () => {
  localPlatform.value = detectPlatform()
  try {
    const r = await api.get('/api/downloads/releases')
    items.value = r.items ?? []
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="dl">
    <header class="dl-head">
      <h1>{{ t('download.title') }}</h1>
      <p class="muted">{{ t('download.subtitle') }}</p>
    </header>

    <div v-if="error" class="err">{{ error }}</div>
    <div v-else-if="loading" class="empty">{{ t('common.loading') }}</div>
    <div v-else-if="!latest" class="empty">{{ t('download.empty') }}</div>

    <template v-else>
      <p class="dl-version">
        <span class="mono">v{{ latest.version }}</span>
        <span v-if="latest.publishedAt" class="muted small">{{ when(latest.publishedAt) }}</span>
      </p>
      <p v-if="latest.notes" class="dl-notes muted small">{{ latest.notes }}</p>

      <div class="dl-grid">
        <article
          v-for="p in orderedPlatforms"
          :key="p.id"
          class="dl-card"
          :class="{ featured: p.id === localPlatform }"
        >
          <section class="dl-sec">
            <div class="dl-title">
              <span class="dl-icon">
                <svg v-if="p.id === 'windows'" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
                  <path d="M3 5.5L10.5 4.3v7.4H3V5.5zm0 8.8h7.5v7.4L3 20.5V14.3zm9.8-10.2L21 2.7v7.3h-8.2V4.1zm0 8.8H21v7.3l-8.2-1.4v-5.9z" />
                </svg>
                <svg v-else-if="p.id === 'macos'" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
                  <path d="M18.7 19.5c-.8.9-1.7 1.6-2.8 1.6-1.1 0-1.4-.7-2.6-.7s-1.5.7-2.6.7c-1.1 0-2-.7-2.8-1.6C4.8 17.8 4 14.9 4.5 12.2c.5-2.2 2-3.7 3.8-3.7 1.1 0 2 .7 2.6.7.6 0 1.5-.8 2.7-.8 1 0 2.5.5 3.3 2-.1.1-2 1.2-2 3.6 0 2.8 2.2 3.7 2.3 3.8-.2.5-.4.9-.7 1.3zM15.5 4.2c.6-.7 1-1.7.9-2.7-1 .1-2.1.6-2.8 1.3-.6.6-1.1 1.6-1 2.6 1.1.1 2.2-.5 2.9-1.2z" />
                </svg>
                <svg v-else viewBox="0 0 24 24" fill="currentColor" aria-hidden>
                  <path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10 10-4.5 10-10S17.5 2 12 2zm0 1.8c1.6 0 3 .4 4.3 1.1L7.9 18.3A8.2 8.2 0 0 1 12 3.8zM5.7 7.7A8.2 8.2 0 0 0 5.7 16l8.6-8.6A8.2 8.2 0 0 0 5.7 7.7z" />
                </svg>
              </span>
              <h2>{{ p.name }}</h2>
              <span v-if="p.id === localPlatform" class="dl-badge">{{ t('download.local') }}</span>
            </div>

            <div class="dl-files">
              <div
                v-for="(a, i) in p.artifacts"
                :key="a.id"
                class="dl-file"
                :class="{ bordered: i > 0 }"
              >
                <div class="dl-file-info">
                  <p class="dl-filename mono" :title="a.filename">{{ a.filename }}</p>
                  <p v-if="sizeMb(a.size)" class="dl-size">
                    {{ t('download.fileSize') }}：{{ sizeMb(a.size) }}
                  </p>
                </div>
                <a
                  :href="a.downloadUrl"
                  class="dl-btn"
                  :class="{ primary: p.id === localPlatform && i === 0 }"
                  :download="a.filename"
                >{{ t('download.download') }}</a>
              </div>

              <div v-if="!p.artifacts.length" class="dl-file">
                <div class="dl-file-info">
                  <p class="dl-filename muted">{{ t('download.unavailable') }}</p>
                </div>
              </div>
            </div>
          </section>

          <section class="dl-sec dl-meta">
            <p class="dl-label">{{ t('download.minReq') }}</p>
            <p class="dl-text">{{ p.minVersion }}</p>
          </section>

          <section class="dl-sec dl-meta">
            <p class="dl-label">{{ t('download.installSteps') }}</p>
            <ul class="dl-text">
              <li v-for="s in p.steps" :key="s">{{ s }}</li>
            </ul>
          </section>
        </article>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dl { max-width: 1080px; margin: 0 auto; }

.dl-head { text-align: center; margin: 8px 0 28px; }
.dl-head h1 { margin: 0; font-size: 24px; font-weight: 600; letter-spacing: -0.01em; }
.dl-head p { margin: 8px 0 0; font-size: 13px; }

.dl-version {
  display: flex;
  align-items: baseline;
  justify-content: center;
  gap: 10px;
  margin: 0 0 4px;
  font-size: 13px;
}
.dl-notes { text-align: center; white-space: pre-wrap; margin: 0 0 20px; }

.dl-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  align-items: stretch;
}
@media (max-width: 900px) {
  .dl-grid { grid-template-columns: 1fr; }
}

.dl-card {
  display: flex;
  flex-direction: column;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 14px;
  overflow: hidden;
}
/* 本机平台：蓝色描边 + 浅色渐变，和官网的 featured 卡一个意思 */
.dl-card.featured {
  border: 2px solid var(--accent);
  box-shadow: 0 12px 32px -12px rgb(37 99 235 / 25%);
}

.dl-sec { padding: 18px; }
.dl-sec + .dl-sec { border-top: 1px solid var(--line); }

.dl-title { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.dl-title h2 { margin: 0; font-size: 16px; font-weight: 600; }
.dl-icon {
  width: 36px; height: 36px; flex: 0 0 36px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 10px;
  background: var(--panel-alt);
  color: var(--muted);
}
.dl-card.featured .dl-icon { background: var(--accent-soft); color: var(--accent); }
.dl-icon svg { width: 20px; height: 20px; }
.dl-badge {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 500;
}

.dl-files { margin-top: 16px; display: flex; flex-direction: column; gap: 10px; }
.dl-file { display: flex; align-items: center; gap: 12px; }
.dl-file.bordered { border-top: 1px solid var(--line); padding-top: 10px; }
.dl-file-info { min-width: 0; flex: 1; }
.dl-filename {
  margin: 0;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dl-size { margin: 2px 0 0; font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }

.dl-btn {
  flex: 0 0 auto;
  font-size: 12px;
  padding: 6px 14px;
  border-radius: 999px;
  border: 1px solid var(--line);
  background: var(--panel);
  color: var(--fg);
  white-space: nowrap;
}
.dl-btn:hover { border-color: var(--accent); color: var(--accent); }
.dl-btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
.dl-btn.primary:hover { opacity: 0.9; color: #fff; }

.dl-meta { display: flex; flex-direction: column; justify-content: center; }
.dl-label {
  margin: 0 0 4px;
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.04em;
  color: var(--muted);
}
.dl-text { margin: 0; font-size: 13px; line-height: 1.7; color: var(--fg); }
.dl-text li { margin: 0; }
</style>
