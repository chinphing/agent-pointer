export interface ExperienceListItem {
  id: string
  slug: string
  title: string
  excerpt: string
  pinned?: boolean
}

export interface ExperienceDetail {
  id: string
  slug: string
  title: string
  prompt_text: string
  narrative_text?: string | null
}
