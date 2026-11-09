<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { useNotice } from '../lib/notice'
import { can } from '../store'

const { t } = useI18n()
const items = ref<{ model: string; promptYuanPerMillion: number; completionYuanPerMillion: number }[]>([])
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()

const draft = ref({ model: '', prompt: '1', completion: '4' })
const saving = ref(false)

async function load() {
  loading.value = true
  error.value = ''
  try {
    const res = await api.get('/api/admin/model-prices')
    items.value = res.items || []
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

async function save(model: string, prompt: string, completion: string) {
  error.value = ''
  saving.value = true
  try {
    await api.post('/api/admin/model-prices', {
      model,
      promptYuanPerMillion: Number(prompt),
      completionYuanPerMillion: Number(completion),
    })
    flashNotice(t('billing.saved', { model }))
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    saving.value = false
  }
}

async function addPrice() {
  const model = draft.value.model.trim()
  if (!model) {
    error.value = t('billing.modelRequired')
    return
  }
  await save(model, draft.value.prompt, draft.value.completion)
  draft.value = { model: '', prompt: '1', completion: '4' }
}

function editPrice(row: any) {
  const p = window.prompt(
    t('billing.promptInput', { model: row.model }),
    String(row.promptYuanPerMillion)
  )
  if (p === null) return
  const c = window.prompt(
    t('billing.completionInput', { model: row.model }),
    String(row.completionYuanPerMillion)
  )
  if (c === null) return
  void save(row.model, p, c)
}

onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <h2>{{ t('nav.billing') }}</h2>
      <p class="muted small">{{ t('billing.hint') }}</p>
    </div>

    <div v-if="notice" class="okmsg" @click="clearNotice">{{ notice }}</div>
    <div v-if="error" class="err">{{ error }}</div>

    <div v-if="can('usage:write')" class="panel">
      <h2>{{ t('billing.addTitle') }}</h2>
      <div class="row wrap">
        <input v-model="draft.model" :placeholder="t('billing.modelPlaceholder')" />
        <input v-model="draft.prompt" class="num" :placeholder="t('billing.prompt')" />
        <input v-model="draft.completion" class="num" :placeholder="t('billing.completion')" />
        <button :disabled="saving" @click="addPrice">{{ t('billing.save') }}</button>
      </div>
      <p class="muted small">{{ t('billing.unitHint') }}</p>
    </div>

    <div class="panel">
      <h2>{{ t('billing.listTitle') }}</h2>
      <p v-if="loading" class="muted">{{ t('common.loading') }}</p>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr>
            <th>{{ t('billing.model') }}</th>
            <th class="num">{{ t('billing.prompt') }}</th>
            <th class="num">{{ t('billing.completion') }}</th>
            <th class="actions"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in items" :key="row.model">
            <td class="mono">{{ row.model }}</td>
            <td class="num mono">{{ row.promptYuanPerMillion }}</td>
            <td class="num mono">{{ row.completionYuanPerMillion }}</td>
            <td class="actions">
              <button v-if="can('usage:write')" class="tiny" @click="editPrice(row)">
                {{ t('common.edit') }}
              </button>
            </td>
          </tr>
          <tr v-if="!items.length">
            <td colspan="4" class="muted small">{{ t('billing.empty') }}</td>
          </tr>
        </tbody>
      </table>
      <p class="muted small">{{ t('billing.fallbackHint') }}</p>
    </div>
  </div>
</template>
