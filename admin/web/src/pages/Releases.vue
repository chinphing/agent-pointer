<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { humanSize, when } from '../lib/format'
import {
  classifyReleaseFile,
  PLATFORM_KINDS,
  signatureTarget,
  type PlatformId,
} from '../lib/releaseFiles'

const { t } = useI18n()

interface Artifact {
  id: string
  kind: string
  filename: string
  size: number
  has_signature: boolean
  created_at: number
}
interface Release {
  id: string
  version: string
  notes: string
  published: boolean
  published_at: number | null
  created_at: number
  artifacts: Artifact[]
}

const releases = ref<Release[]>([])
const loading = ref(true)
const error = ref('')
const notice = ref('')

const showCreate = ref(false)
const newVersion = ref('')
const newNotes = ref('')
const creating = ref(false)
const createError = ref('')

/**
 * 一个平台一个上传区：选中该平台的全部产物（安装包 + 更新包 + .sig）一次传完，
 * 不再给每个类型单独摆一个上传框。归类规则见 `lib/releaseFiles.ts`。
 */
const PLATFORMS: { id: PlatformId; labelKey: string }[] = [
  { id: 'macos', labelKey: 'download.macos' },
  { id: 'windows', labelKey: 'download.windows' },
  { id: 'linux', labelKey: 'download.linux' },
]

const slots = ref<Record<string, { files: File[]; busy: boolean; error: string }>>({})

function slot(platform: PlatformId) {
  if (!slots.value[platform]) slots.value[platform] = { files: [], busy: false, error: '' }
  return slots.value[platform]
}

/**
 * 文件名 → 产物类型。平台决定更新包归属，扩展名决定安装包格式。
 * 认不出来的返回 null，由调用方报「跳过」。
 */
function classify(name: string, platform: PlatformId): string | null {
  const n = name.toLowerCase()
  // 先判压缩包：`.AppImage.tar.gz` 是更新包，不是 AppImage 安装包
  if (n.endsWith('.tar.gz') || n.endsWith('.zip')) {
    if (platform === 'macos') return 'macos_updater'
    if (platform === 'windows') return 'windows_updater'
    return 'linux_appimage_updater'
  }
  if (n.endsWith('.dmg')) return 'macos_dmg'
  if (n.endsWith('.msi')) return 'windows_msi'
  if (n.endsWith('.exe')) return 'windows_nsis'
  if (n.endsWith('.deb')) return 'linux_deb'
  if (n.endsWith('.appimage')) return 'linux_appimage'
  return null
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const res = await api.get('/api/admin/releases')
    releases.value = res.items || []
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)

const hasAny = computed(() => releases.value.length > 0)

async function create() {
  creating.value = true
  createError.value = ''
  try {
    await api.post('/api/admin/releases', { version: newVersion.value, notes: newNotes.value })
    showCreate.value = false
    newVersion.value = ''
    newNotes.value = ''
    notice.value = t('releases.created')
    await load()
  } catch (e) {
    createError.value = errText(e)
  } finally {
    creating.value = false
  }
}

/** 产物类型的中文名；未知类型原样显示。 */
const KIND_KEYS: Record<string, string> = {
  macos_dmg: 'releases.kindMacosDmg',
  windows_msi: 'releases.kindWindowsMsi',
  windows_nsis: 'releases.kindWindowsNsis',
  linux_deb: 'releases.kindLinuxDeb',
  linux_appimage: 'releases.kindLinuxAppImage',
  macos_updater: 'releases.kindMacosUpdater',
  windows_updater: 'releases.kindWindowsUpdater',
  linux_appimage_updater: 'releases.kindLinuxUpdater',
}
function kindLabel(kind: string): string {
  const key = KIND_KEYS[kind]
  return key ? t(key) : kind
}

/** 列表按创建时间倒序，所以「上一版」就是数组里的下一条。 */
function previousOf(r: Release): Release | null {
  const i = releases.value.findIndex((x) => x.id === r.id)
  return i >= 0 && i + 1 < releases.value.length ? releases.value[i + 1] : null
}

const reuseBusy = ref('')

/** 沿用上一版的包：只复制记录，不重传文件。 */
async function reuseFromPrevious(r: Release) {
  const prev = previousOf(r)
  if (!prev) return
  reuseBusy.value = r.id
  error.value = ''
  try {
    const res = await api.post(`/api/admin/releases/${r.id}/artifacts/reuse`, {
      from: prev.id,
    })
    const n = (res.copied || []).length
    notice.value = n
      ? t('releases.reuseDone', { n, version: prev.version })
      : t('releases.reuseNothing')
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    reuseBusy.value = ''
  }
}

function artifactOf(r: Release, kind: string) {
  return r.artifacts.find((a) => a.kind === kind) || null
}

/** 该平台下已上传的产物。 */
function platformArtifacts(r: Release, platform: PlatformId) {
  return PLATFORM_KINDS[platform]
    .map((kind) => artifactOf(r, kind))
    .filter((a): a is Artifact => a !== null)
}

/** 该平台还没传的类型。 */
function missingKinds(r: Release, platform: PlatformId) {
  return PLATFORM_KINDS[platform].filter((k) => !artifactOf(r, k))
}

/** 选中一批文件，按文件名归位后逐个上传。 */
async function uploadPlatform(r: Release, platform: PlatformId) {
  const s = slot(platform)
  s.error = ''
  if (!s.files.length) {
    s.error = t('releases.pickFiles')
    return
  }

  // .sig 按「归档名 + .sig」配对：foo.app.tar.gz 的签名是 foo.app.tar.gz.sig
  const sigs = new Map<string, File>()
  const archives: File[] = []
  for (const f of s.files) {
    const target = signatureTarget(f.name)
    if (target !== null) sigs.set(target, f)
    else archives.push(f)
  }
  if (!archives.length) {
    s.error = t('releases.onlySig')
    return
  }

  s.busy = true
  const ok: string[] = []
  const skipped: string[] = []
  try {
    for (const f of archives) {
      const kind = classifyReleaseFile(f.name, platform)
      if (!kind || !PLATFORM_KINDS[platform].includes(kind)) {
        skipped.push(f.name)
        continue
      }
      // 更新包必须带签名；.sig 原样全文传，客户端验签用的就是这段字节
      const sig = sigs.get(f.name)
      if (kind.endsWith('_updater') && !sig) {
        skipped.push(`${f.name}（缺 .sig）`)
        continue
      }
      const form = new FormData()
      form.append('kind', kind)
      form.append('signature', sig ? await sig.text() : '')
      form.append('file', f, f.name)
      await api.upload(`/api/admin/releases/${r.id}/artifacts`, form)
      ok.push(f.name)
    }
    s.files = []
    notice.value = skipped.length
      ? t('releases.uploadPartial', { ok: ok.length, skipped: skipped.join('、') })
      : t('releases.uploadCount', { n: ok.length })
    await load()
  } catch (e) {
    s.error = errText(e)
  } finally {
    s.busy = false
  }
}

async function togglePublish(r: Release) {
  try {
    await api.post(`/api/admin/releases/${r.id}/publish`, { published: !r.published })
    notice.value = r.published ? t('releases.unpublished') : t('releases.published')
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function remove(r: Release) {
  if (!confirm(t('releases.confirmDelete', { version: r.version }))) return
  try {
    await api.del(`/api/admin/releases/${r.id}`)
    notice.value = t('releases.deleted')
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function removeArtifact(r: Release, kind: string) {
  try {
    await api.del(`/api/admin/releases/${r.id}/artifacts/${kind}`)
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

function onPick(platform: PlatformId, ev: Event) {
  const input = ev.target as HTMLInputElement
  slot(platform).files = Array.from(input.files || [])
  slot(platform).error = ''
}
</script>

<template>
  <div class="spread">
    <div>
      <p class="muted small" style="margin: 0">{{ t('releases.subtitle') }}</p>
    </div>
    <div class="row">
      <button  @click="load">{{ t('common.refresh') }}</button>
      <button class="primary" @click="showCreate = true">{{ t('releases.newVersion') }}</button>
    </div>
  </div>

  <p v-if="notice" class="okmsg">{{ notice }}</p>
  <p v-if="error" class="err">{{ error }}</p>

  <p v-if="loading" class="muted">{{ t('common.loading') }}</p>
  <p v-else-if="!hasAny" class="empty">{{ t('releases.empty') }}</p>

  <div v-for="r in releases" :key="r.id" class="panel" style="margin-top: 14px">
    <div class="spread">
      <div class="row">
        <strong>{{ r.version }}</strong>
        <span class="tag" :class="r.published ? 'ok' : 'plain'">
          {{ r.published ? t('releases.statusPublished') : t('releases.statusDraft') }}
        </span>
        <span class="muted small">
          {{ r.published && r.published_at ? when(r.published_at) : when(r.created_at) }}
        </span>
      </div>
      <div class="row">
        <!-- 只改了部分平台时，其余包直接沿用上一版，不用重传 -->
        <button
          v-if="previousOf(r)"
          :disabled="reuseBusy === r.id"
          :title="t('releases.reuseHint', { version: previousOf(r)!.version })"
          @click="reuseFromPrevious(r)"
        >
          {{ reuseBusy === r.id ? t('releases.reusing') : t('releases.reusePrevious') }}
        </button>
        <button  @click="togglePublish(r)">
          {{ r.published ? t('releases.unpublish') : t('releases.publish') }}
        </button>
        <button class="btn danger" @click="remove(r)">{{ t('common.delete') }}</button>
      </div>
    </div>

    <p v-if="r.notes" class="small" style="margin: 8px 0 0">{{ r.notes }}</p>

    <!-- 一个平台一个上传区：安装包、更新包、.sig 一次选完，按文件名自动归位 -->
    <div v-for="p in PLATFORMS" :key="p.id" class="plat">
      <div class="plat-head">
        <strong class="plat-name">{{ t(p.labelKey) }}</strong>
        <span v-if="missingKinds(r, p.id).length" class="tag plain">
          {{ t('releases.missingCount', { n: missingKinds(r, p.id).length }) }}
        </span>
        <span v-else class="tag ok">{{ t('releases.complete') }}</span>
      </div>

      <ul class="plat-list">
        <li v-for="kind in PLATFORM_KINDS[p.id]" :key="kind" class="plat-row">
          <span class="plat-kind">{{ kindLabel(kind) }}</span>
          <template v-if="artifactOf(r, kind)">
            <span class="plat-file mono" :title="artifactOf(r, kind)!.filename">
              {{ artifactOf(r, kind)!.filename }}
            </span>
            <span class="plat-size">{{ humanSize(artifactOf(r, kind)!.size) }}</span>
            <span v-if="artifactOf(r, kind)!.has_signature" class="tag ok">
              {{ t('releases.signed') }}
            </span>
            <span v-else-if="kind.endsWith('_updater')" class="tag plain">
              {{ t('releases.unsigned') }}
            </span>
            <span v-else></span>
            <button class="tiny ghost" @click="removeArtifact(r, kind)">
              {{ t('common.delete') }}
            </button>
          </template>
          <template v-else>
            <span class="plat-file muted">{{ t('releases.notUploaded') }}</span>
            <span class="plat-size"></span>
            <span></span>
            <span></span>
          </template>
        </li>
      </ul>

      <div class="plat-upload">
        <input
          type="file"
          multiple
          class="plat-input"
          :title="t('releases.pickHint')"
          @change="onPick(p.id, $event)"
        />
        <span v-if="slot(p.id).files.length" class="plat-picked small muted">
          {{ slot(p.id).files.map((f) => f.name).join('、') }}
        </span>
        <button class="tiny" :disabled="slot(p.id).busy" @click="uploadPlatform(r, p.id)">
          {{ slot(p.id).busy ? t('releases.uploading') : t('releases.upload') }}
        </button>
      </div>

      <p v-if="slot(p.id).error" class="err small" style="margin: 8px 0 0">{{ slot(p.id).error }}</p>
    </div>
  </div>

  <div v-if="showCreate" class="modal-mask" @click.self="showCreate = false">
    <div class="modal" style="width: 480px">
      <h2>{{ t('releases.newVersion') }}</h2>
      <label>{{ t('releases.version') }}</label>
      <input v-model="newVersion" placeholder="1.2.3" />
      <label style="margin-top: 10px">{{ t('releases.notes') }}</label>
      <textarea v-model="newNotes" rows="3" :placeholder="t('releases.notesPlaceholder')"></textarea>
      <p v-if="createError" class="err">{{ createError }}</p>
      <div class="row" style="justify-content: flex-end; margin-top: 14px">
        <button  @click="showCreate = false">{{ t('common.cancel') }}</button>
        <button class="primary" :disabled="creating || !newVersion.trim()" @click="create">
          {{ t('common.create') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 每个平台一张子卡片：平台名 + 状态标签 + 逐行产物 + 一行上传 */
.plat {
  margin-top: 14px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel-alt);
}
.plat-head { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
.plat-name { font-size: 13px; }

.plat-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.plat-row {
  display: grid;
  grid-template-columns: 96px minmax(0, 1fr) 80px auto 52px;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  min-height: 22px;
}
.plat-kind { color: var(--muted); }
.plat-file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.plat-size { color: var(--muted); text-align: right; font-variant-numeric: tabular-nums; }
.plat-row .ghost {
  justify-self: end;
  border-color: transparent;
  background: transparent;
  color: var(--muted);
}
.plat-row .ghost:hover { color: var(--danger); border-color: var(--line); }

.plat-upload {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px dashed var(--line);
}
.plat-input { max-width: 230px; font-size: 12px; }
.plat-picked {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
