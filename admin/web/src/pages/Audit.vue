<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { prettyJson as pretty, when } from '../lib/format'

const items = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const limit = ref(100)

async function load() {
  loading.value = true
  try {
    const res = await api.get(`/api/admin/audit?limit=${limit.value}`)
    items.value = res.items
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)
</script>

<template>
  <div class="spread">
    <div>
      <h1>审计</h1>
      <p class="muted small" style="margin: 4px 0 0">所有写操作都会留痕，含改前改后。</p>
    </div>
    <div class="row">
      <select v-model.number="limit" style="width: 110px" @change="load">
        <option :value="50">最近 50</option>
        <option :value="100">最近 100</option>
        <option :value="200">最近 200</option>
        <option :value="500">最近 500</option>
      </select>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>

  <div class="panel" style="margin-top: 16px">
    <div v-if="loading" class="empty">加载中…</div>
    <div v-else-if="!items.length" class="empty">还没有审计记录</div>
    <table v-else>
      <thead>
        <tr>
          <th style="width: 160px">时间</th>
          <th style="width: 120px">操作人</th>
          <th style="width: 170px">动作</th>
          <th>对象</th>
          <th style="width: 90px"></th>
        </tr>
      </thead>
      <tbody>
        <template v-for="entry in items" :key="entry.id">
          <tr>
            <td class="small">{{ when(entry.createdAt) }}</td>
            <td class="small">{{ entry.actorName || '—' }}</td>
            <td><span class="tag plain mono">{{ entry.action }}</span></td>
            <td class="small">
              <span class="muted">{{ entry.targetType }}</span>
              <span class="mono" style="margin-left: 6px">{{ entry.targetId || '—' }}</span>
            </td>
            <td>
              <details v-if="entry.before || entry.after">
                <summary class="small" style="cursor: pointer">详情</summary>
              </details>
            </td>
          </tr>
          <tr v-if="entry.before || entry.after">
            <td colspan="5" style="background: #fafbfc">
              <div v-if="entry.before" class="small">
                <div class="muted">改前</div>
                <pre class="mono">{{ pretty(entry.before) }}</pre>
              </div>
              <div v-if="entry.after" class="small">
                <div class="muted">改后</div>
                <pre class="mono">{{ pretty(entry.after) }}</pre>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
pre {
  margin: 4px 0 10px;
  padding: 8px 10px;
  background: #fff;
  border: 1px solid var(--line);
  border-radius: 8px;
  overflow: auto;
  max-height: 220px;
}
</style>
