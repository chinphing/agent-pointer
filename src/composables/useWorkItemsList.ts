import { ref, watch, type Ref } from 'vue'
import { getWorkItemStats, listWorkItems } from '../lib/api'

export const WORK_ITEMS_PAGE_SIZE = 50

export interface WorkItemRow {
  id: string
  batchId: string
  seq: number
  title: string
  status: string
  resultSummary: string
}

export interface WorkItemBatchStats {
  total: number
  done: number
  failed: number
  inProgress: number
  pending?: number
}

/** @internal exported for unit tests */
export function parseResultSummary(raw: unknown): string {
  if (raw == null) return ''
  if (typeof raw === 'string') {
    const t = raw.trim()
    if (!t) return ''
    try {
      const v = JSON.parse(t) as { summary?: string }
      if (typeof v.summary === 'string') return v.summary.trim()
    } catch {
      return t.length > 80 ? `${t.slice(0, 80)}…` : t
    }
    return t.length > 80 ? `${t.slice(0, 80)}…` : t
  }
  if (typeof raw === 'object' && raw !== null && 'summary' in raw) {
    const s = (raw as { summary?: unknown }).summary
    return typeof s === 'string' ? s.trim() : ''
  }
  return ''
}

/** @internal exported for unit tests */
export function mapListResponse(raw: Record<string, unknown>): { items: WorkItemRow[]; total: number } {
  const total = typeof raw.total === 'number' ? raw.total : 0
  const rows = Array.isArray(raw.items) ? raw.items : []
  const items: WorkItemRow[] = rows.map(row => {
    const r = row as Record<string, unknown>
    return {
      id: String(r.id ?? ''),
      batchId: String(r.batch_id ?? ''),
      seq: typeof r.seq === 'number' ? r.seq : 0,
      title: String(r.title ?? '').trim() || String(r.id ?? ''),
      status: String(r.status ?? 'pending'),
      resultSummary: parseResultSummary(r.result_summary)
    }
  })
  return { items, total }
}

function mapStatsResponse(raw: Record<string, unknown>): WorkItemBatchStats {
  return {
    total: typeof raw.total === 'number' ? raw.total : 0,
    done: typeof raw.done === 'number' ? raw.done : 0,
    failed: typeof raw.failed === 'number' ? raw.failed : 0,
    inProgress: typeof raw.in_progress === 'number' ? raw.in_progress : 0,
    pending: typeof raw.pending === 'number' ? raw.pending : undefined
  }
}

export function useWorkItemsList(opts: {
  conversationId: Ref<string | null | undefined>
  taskId: Ref<string | undefined>
  batchId: Ref<string>
  enabled: Ref<boolean>
  refreshKey: Ref<string>
}) {
  const items = ref<WorkItemRow[]>([])
  const total = ref(0)
  const offset = ref(0)
  const stats = ref<WorkItemBatchStats | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const expanded = ref(false)

  async function loadStats() {
    const convId = opts.conversationId.value?.trim()
    if (!convId || !opts.enabled.value) return
    try {
      const raw = await getWorkItemStats(convId, {
        taskId: opts.taskId.value,
        batchId: opts.batchId.value
      })
      stats.value = mapStatsResponse(raw)
    } catch (e) {
      stats.value = null
      error.value = e instanceof Error ? e.message : String(e)
    }
  }

  async function loadPage(nextOffset = offset.value) {
    const convId = opts.conversationId.value?.trim()
    if (!convId || !opts.enabled.value || !expanded.value) return
    loading.value = true
    error.value = null
    try {
      const raw = await listWorkItems(convId, {
        taskId: opts.taskId.value,
        batchId: opts.batchId.value,
        offset: nextOffset,
        limit: WORK_ITEMS_PAGE_SIZE
      })
      const parsed = mapListResponse(raw)
      items.value = parsed.items
      total.value = parsed.total
      offset.value = nextOffset
    } catch (e) {
      items.value = []
      total.value = 0
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  }

  function setExpanded(open: boolean) {
    expanded.value = open
    if (open) {
      void loadStats()
      void loadPage(0)
    }
  }

  function nextPage() {
    const next = offset.value + WORK_ITEMS_PAGE_SIZE
    if (next >= total.value) return
    void loadPage(next)
  }

  function prevPage() {
    const next = Math.max(0, offset.value - WORK_ITEMS_PAGE_SIZE)
    void loadPage(next)
  }

  watch(
    () => [opts.refreshKey.value, opts.enabled.value] as const,
    () => {
      if (expanded.value) {
        void loadStats()
        void loadPage(offset.value)
      }
    }
  )

  return {
    items,
    total,
    offset,
    stats,
    loading,
    error,
    expanded,
    setExpanded,
    loadStats,
    loadPage,
    nextPage,
    prevPage,
    pageSize: WORK_ITEMS_PAGE_SIZE
  }
}
