import type {
  ExperienceDetail,
  ExperienceHomeResponse,
  ExperienceListItem,
} from '../types/experience'
import { isTauriRuntime } from './runtime'
import { invoke } from '@tauri-apps/api/core'
import { WEB_API_BASE } from './runtime'

const REQUEST_TIMEOUT_MS = 12_000

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
