import { onScopeDispose, ref } from 'vue'

/**
 * 成功提示：显示几秒后自动消失。
 *
 * 原来是各页面自己 `notice.value = 'xxx'`，设完就一直挂着——切到别的页签
 * 还留在那儿，看着像当前状态。统一走这里，顺便把定时器收干净。
 */
export function useNotice(ttlMs = 3000) {
  const notice = ref('')
  let timer: ReturnType<typeof setTimeout> | null = null

  function clear() {
    notice.value = ''
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
  }

  function flash(text: string) {
    notice.value = text
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      notice.value = ''
      timer = null
    }, ttlMs)
  }

  onScopeDispose(clear)

  return { notice, flash, clear }
}
