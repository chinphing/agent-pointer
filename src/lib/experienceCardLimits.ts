/** APP 经验卡片展示字数上限（须与官网 API `experience_card_limits.py` 一致） */

export const EXPERIENCE_CARD_TITLE_MAX = 48
export const EXPERIENCE_CARD_NARRATIVE_MAX = 120

export function truncateExperienceCardTitle(title: string): string {
  const t = title.trim()
  if (t.length <= EXPERIENCE_CARD_TITLE_MAX) return t
  return `${t.slice(0, EXPERIENCE_CARD_TITLE_MAX - 1)}…`
}

export function truncateExperienceCardExcerpt(text: string): string {
  const t = text.replace(/\s+/g, ' ').trim()
  if (t.length <= EXPERIENCE_CARD_NARRATIVE_MAX) return t
  return `${t.slice(0, EXPERIENCE_CARD_NARRATIVE_MAX - 1)}…`
}
