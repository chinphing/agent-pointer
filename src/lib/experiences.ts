import type {
  ExperienceDetail,
  ExperienceHomeResponse,
  ExperienceListItem,
} from '../types/experience'
import { truncateExperienceCardExcerpt } from './experienceCardLimits'
import { isTauriRuntime } from './runtime'
import { invoke } from '@tauri-apps/api/core'
import { WEB_API_BASE } from './runtime'

export { EXPERIENCE_CARD_NARRATIVE_MAX, EXPERIENCE_CARD_TITLE_MAX } from './experienceCardLimits'
export { truncateExperienceCardExcerpt, truncateExperienceCardTitle } from './experienceCardLimits'

const REQUEST_TIMEOUT_MS = 12_000

/** Card preview: collapse numbered lists, then apply narrative length cap. */
export function formatExperienceCardExcerpt(raw: string | null | undefined): string {
  const text = (raw ?? '').replace(/\s+/g, ' ').trim()
  if (!text) return ' '
  const numbered = text.match(/^1[.)．、]\s*(.+?)(?:\s+2[.)．、]\s|$)/)
  const normalized = numbered ? numbered[1].trim() : text
  return truncateExperienceCardExcerpt(normalized)
}

async function webRequest<T>(path: string): Promise<T> {
  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS)
  try {
    const res = await fetch(`${WEB_API_BASE}${path}`, { signal: controller.signal })
    if (!res.ok) throw new Error(await res.text())
    return await res.json()
  } finally {
    window.clearTimeout(timeoutId)
  }
}

/** @deprecated use listExperienceHome */
export async function listPinnedExperiences(limit = 3): Promise<ExperienceListItem[]> {
  if (isTauriRuntime()) {
    return await invoke<ExperienceListItem[]>('list_pinned_experiences', { limit })
  }
  return await webRequest<ExperienceListItem[]>(`/api/experiences/pinned?limit=${limit}`)
}

export async function listExperienceHome(): Promise<ExperienceHomeResponse> {
  if (isTauriRuntime()) {
    return await invoke<ExperienceHomeResponse>('list_experience_home')
  }
  return await webRequest<ExperienceHomeResponse>('/api/experiences/home')
}

export async function searchExperiences(query: string, limit = 20): Promise<ExperienceListItem[]> {
  const q = query.trim()
  if (!q) return []
  if (isTauriRuntime()) {
    return await invoke<ExperienceListItem[]>('search_experiences', { query: q, limit })
  }
  return await webRequest<ExperienceListItem[]>(
    `/api/experiences/search?q=${encodeURIComponent(q)}&limit=${limit}`,
  )
}

export async function getExperienceDetail(slug: string): Promise<ExperienceDetail> {
  if (isTauriRuntime()) {
    return await invoke<ExperienceDetail>('get_experience_detail', { slug })
  }
  return await webRequest<ExperienceDetail>(`/api/experiences/${encodeURIComponent(slug)}`)
}
