function pad2(n: number): string {
  return n.toString().padStart(2, '0')
}

/** 不显示小时：分:秒（当日时钟内的分与秒） */
export function formatMessageTimeMinuteSecond(ts: number): string {
  const d = new Date(ts)
  return `${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`
}

/** Full local time for tooltip: YYYY-MM-DD HH:mm:ss */
export function formatMessageTimeFull(ts: number): string {
  const d = new Date(ts)
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`
}
