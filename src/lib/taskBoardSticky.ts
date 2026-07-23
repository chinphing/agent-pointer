export function shouldStickActiveTaskBoard(
  scrollTop: number,
  inlineScrollTop: number | null,
  isActive: boolean
): boolean {
  return isActive && inlineScrollTop != null && scrollTop >= inlineScrollTop
}
