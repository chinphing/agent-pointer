<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { humanSize, when } from '../lib/format'

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
const kinds = ref<string[]>([])
const loading = ref(true)
const error = ref('')
const notice = ref('')

const showCreate = ref(false)
const newVersion = ref('')
const newNotes = ref('')
const creating = ref(false)
const createError = ref('')

/** 每个平台一个上传槽：安装包 + 签名文件（.sig 读成文本，后端要求内联全文）。 */
const slots = ref<Record<string, { file: File | null; sig: File | null; busy: boolean; error: string }>>({})

function slot(kind: string) {
  if (!slots.value[kind]) slots.value[kind] = { file: null, sig: null, busy: false, error: '' }
  return slots.value[kind]
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const res = await api.get('/api/admin/releases')
    releases.value = res.items || []
    kinds.value = res.kinds || []
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

function artifactOf(r: Release, kind: string) {
  return r.artifacts.find((a) => a.kind === kind) || null
}

async function upload(r: Release, kind: string) {
  const s = slot(kind)
  s.error = ''
  if (!s.file) {
    s.error = t('releases.pickPackage')
    return
  }
  if (!s.sig) {
    s.error = t('releases.pickSignature')
    return
  }
  s.busy = true
  try {
    // .sig 必须原样全文传给后端，客户端验签用的就是这段字节
    const signature = await s.sig.text()
    const form = new FormData()
    form.append('kind', kind)
    form.append('signature', signature)
    form.append('file', s.file, s.file.name)
    await api.upload(`/api/admin/releases/${r.id}/artifacts`, form)
    s.file = null
    s.sig = null
    notice.value = t('releases.uploaded')
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

function onPick(kind: string, which: 'file' | 'sig', ev: Event) {
  const input = ev.target as HTMLInputElement
  const f = input.files?.[0] || null
  slot(kind)[which] = f
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
        <button  @click="togglePublish(r)">
          {{ r.published ? t('releases.unpublish') : t('releases.publish') }}
        </button>
        <button class="btn danger" @click="remove(r)">{{ t('common.delete') }}</button>
      </div>
    </div>

    <p v-if="r.notes" class="small" style="margin: 8px 0 0">{{ r.notes }}</p>

    <table style="margin-top: 10px">
      <thead>
        <tr>
          <th style="width: 22%">{{ t('releases.platform') }}</th>
          <th style="width: 34%">{{ t('releases.package') }}</th>
          <th style="width: 12%">{{ t('releases.size') }}</th>
          <th>{{ t('releases.signature') }}</th>
          <th style="width: 16%"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="kind in kinds" :key="kind">
          <td><code>{{ kind }}</code></td>
          <td>
            <template v-if="artifactOf(r, kind)">
              {{ artifactOf(r, kind)!.filename }}
            </template>
            <template v-else>
              <input type="file" class="small" @change="onPick(kind, 'file', $event)" />
            </template>
          </td>
          <td class="muted small">
            {{ artifactOf(r, kind) ? humanSize(artifactOf(r, kind)!.size) : '—' }}
          </td>
          <td>
            <template v-if="artifactOf(r, kind)">
              <span v-if="artifactOf(r, kind)!.has_signature" class="tag ok">{{ t('releases.signed') }}</span>
              <span v-else class="tag danger">{{ t('releases.unsigned') }}</span>
            </template>
            <template v-else>
              <input type="file" accept=".sig,text/plain" class="small" @change="onPick(kind, 'sig', $event)" />
            </template>
          </td>
          <td>
            <template v-if="artifactOf(r, kind)">
              <button class="tiny" @click="removeArtifact(r, kind)">{{ t('common.delete') }}</button>
            </template>
            <template v-else>
              <button class="tiny primary" :disabled="slot(kind).busy" @click="upload(r, kind)">
                {{ slot(kind).busy ? t('releases.uploading') : t('releases.upload') }}
              </button>
            </template>
          </td>
        </tr>
      </tbody>
    </table>
    <p v-if="Object.values(slots).some((s) => s.error)" class="err small">
      {{ Object.values(slots).find((s) => s.error)?.error }}
    </p>
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
