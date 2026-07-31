/**
 * 将 HTTP 错误响应体摘要为短文本，避免 nginx HTML 页面或大段 JSON 成为原始错误消息。
 *
 * 策略：
 * 1. 空响应体 → `请求失败 (${status})`
 * 2. HTML → 提取 `<title>`；无 title 则剥标签压空白取前 200 字符
 * 3. JSON → 取 `error || message || detail` 字符串字段
 * 4. 纯文本 → 截取前 500 字符
 *
 * 保留 status code 数字与关键词（如 "413"），确保 retry.ts 的正则匹配仍能命中。
 */
export async function summarizeErrorResponse(res: Response): Promise<string> {
  const status = res.status
  let text = ''
  try {
    text = await res.text()
  } catch {
    // 无法读取响应体时仅返回状态码
    return `请求失败 (${status})`
  }

  if (!text || !text.trim()) {
    return `请求失败 (${status})`
  }

  const lower = text.trimStart().toLowerCase()

  // HTML：提取 <title> 或剥标签
  const contentType = (res.headers.get('content-type') ?? '').toLowerCase()
  if (contentType.includes('text/html') || lower.startsWith('<!doctype') || lower.startsWith('<html')) {
    const titleMatch = text.match(/<title[^>]*>([\s\S]*?)<\/title>/i)
    if (titleMatch?.[1]?.trim()) {
      return `[${status}] ${titleMatch[1].trim().replace(/\s+/g, ' ').slice(0, 200)}`
    }
    const stripped = text.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim()
    if (stripped) {
      return `[${status}] ${stripped.slice(0, 200)}`
    }
    return `请求失败 (${status})`
  }

  // JSON：提取常用错误字段
  if (lower.startsWith('{') || lower.startsWith('[')) {
    try {
      const parsed = JSON.parse(text)
      const detail = parsed?.error ?? parsed?.message ?? parsed?.detail
      if (typeof detail === 'string' && detail.trim()) {
        return `[${status}] ${detail.trim().slice(0, 500)}`
      }
    } catch {
      // 解析失败，fall through 到纯文本截断
    }
  }

  // 纯文本
  const clipped = text.trim().slice(0, 500)
  return `[${status}] ${clipped}`
}
