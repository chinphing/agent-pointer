import { ref, watch, type WatchSource } from 'vue'
import type { RenderableAttachment } from '../lib/messageNormalizer'
import {
  resolveEagerPreviewUrl,
  shouldEagerLoadPreview
} from '../lib/chatAttachmentLoad'
import { prefetchAttachmentAbsPaths, resolveAttachmentAbsPath } from '../lib/attachmentLocalPath'
import { isUsableAttachmentPreviewUrl } from '../lib/attachmentSupport'
import { downloadAttachment } from '../lib/openAttachment'
import { revealInFinder } from '../lib/api'

export function useChatAttachmentDisplay(
  attachmentsSource: WatchSource<RenderableAttachment[]>
) {
  const loadedPreviews = ref<Record<string, string>>({})
  const resolvedAbsPaths = ref<Record<string, string>>({})
  const previewInflight = new Set<string>()

  async function ensureMediaPreview(att: RenderableAttachment) {
    if (loadedPreviews.value[att.id] || previewInflight.has(att.id)) return
    if (isUsableAttachmentPreviewUrl(att.previewUrl)) {
      loadedPreviews.value = {
        ...loadedPreviews.value,
        [att.id]: att.previewUrl!.trim()
      }
      return
    }
    if (!shouldEagerLoadPreview(att)) return

    previewInflight.add(att.id)
    try {
      const url = await resolveEagerPreviewUrl(att)
      if (url) {
        loadedPreviews.value = { ...loadedPreviews.value, [att.id]: url }
      }
    } catch (e) {
      console.warn('attachment preview failed', att.fileName, e)
    } finally {
      previewInflight.delete(att.id)
    }
  }

  function mediaSrc(att: Pick<RenderableAttachment, 'id' | 'previewUrl'>): string | null {
    if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
    return loadedPreviews.value[att.id] ?? null
  }

  function filePath(att: RenderableAttachment): string | undefined {
    return resolvedAbsPaths.value[att.id] ?? att.localAbsPath
  }

  async function onDownloadAttachment(att: RenderableAttachment) {
    try {
      await downloadAttachment(att, mediaSrc(att))
    } catch (e) {
      console.warn('download attachment failed', e)
    }
  }

  async function copyFilePath(att: RenderableAttachment) {
    const path = filePath(att) ?? (await resolveAttachmentAbsPath(att))
    if (!path) return
    try {
      await navigator.clipboard.writeText(path)
    } catch (e) {
      console.warn('copy file path failed', e)
    }
  }

  async function onRevealInFinder(att: RenderableAttachment) {
    const path = filePath(att) ?? (await resolveAttachmentAbsPath(att))
    if (!path) return
    try {
      await revealInFinder(path)
    } catch (e) {
      console.warn('reveal in finder failed', e)
    }
  }

  watch(
    attachmentsSource,
    list => {
      for (const att of list) void ensureMediaPreview(att)
      void prefetchAttachmentAbsPaths(list).then(map => {
        resolvedAbsPaths.value = map
      })
    },
    { immediate: true, deep: true }
  )

  return {
    mediaSrc,
    filePath,
    onDownloadAttachment,
    copyFilePath,
    onRevealInFinder
  }
}
