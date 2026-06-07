export interface ExperienceListItem {
  id: string
  slug: string
  title: string
  excerpt: string
  pinned?: boolean
  featured?: boolean
  category_id?: string | null
  category_name_zh?: string | null
  agent_id?: string
  cover_image_url?: string | null
}

export interface ExperienceDetail {
  id: string
  slug: string
  title: string
  prompt_text: string
  narrative_text?: string | null
  agent_id?: string
  category_id?: string | null
  cover_image_url?: string | null
}

export interface ExperienceHomeCategoryBlock {
  id: string
  name_zh: string
  items: ExperienceListItem[]
}

export interface ExperienceHomeResponse {
  featured: ExperienceListItem[]
  categories: ExperienceHomeCategoryBlock[]
}
