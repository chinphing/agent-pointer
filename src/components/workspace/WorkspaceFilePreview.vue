<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, Copy, FileWarning, WrapText } from 'lucide-vue-next'
import type { WorkspaceFilePreview } from '../../lib/api'
import { tokenizeCodeLine } from '../../lib/workspaceFilePreview'

const props = defineProps<{
  preview: WorkspaceFilePreview
  absolutePath: string
}>()

const wrapLines = ref(false)
const copied = ref(false)
const lines = computed(() => (props.preview.content ?? '').split('\n'))
const language = computed(() => {
  const ext = props.preview.path.split('.').pop()?.toLowerCase() || ''
  const names: Record<string, string> = {
    ts: 'TypeScript', tsx: 'TSX', js: 'JavaScript', jsx: 'JSX', vue: 'Vue',
    rs: 'Rust', py: 'Python', go: 'Go', java: 'Java', kt: 'Kotlin', swift: 'Swift',
    json: 'JSON', jsonc: 'JSONC', toml: 'TOML', yaml: 'YAML', yml: 'YAML',
    md: 'Markdown', css: 'CSS', scss: 'SCSS', html: 'HTML', xml: 'XML',
    sh: 'Shell', sql: 'SQL', txt: 'Text'
  }
  return names[ext] || (ext ? ext.toUpperCase() : 'Text')
})
const sizeLabel = computed(() => {
  const bytes = props.preview.sizeBytes
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
})

async function copyPath() {
  await navigator.clipboard.writeText(props.absolutePath)
  copied.value = true
  window.setTimeout(() => { copied.value = false }, 1200)
}
</script>

<template>
  <div class="file-preview">
    <div class="file-preview-toolbar">
      <span>{{ language }}</span>
      <span>{{ sizeLabel }}</span>
      <span v-if="preview.truncated" class="text-warning">仅显示前 1 MB</span>
      <span class="flex-1" />
      <button type="button" :class="wrapLines && 'is-active'" title="切换自动换行" @click="wrapLines = !wrapLines">
        <WrapText />
      </button>
      <button type="button" title="复制绝对路径" @click="copyPath">
        <Check v-if="copied" /><Copy v-else />
      </button>
    </div>

    <div v-if="preview.binary" class="file-preview-empty">
      <FileWarning class="w-5 h-5" />
      <strong>无法预览二进制文件</strong>
      <span>{{ preview.path }} · {{ sizeLabel }}</span>
    </div>
    <div v-else class="file-preview-scroll">
      <div class="file-preview-code" :class="wrapLines && 'wrap-lines'">
        <div v-for="(line, index) in lines" :key="index" class="file-preview-row">
          <span class="file-preview-line-number">{{ index + 1 }}</span>
          <code>
            <span
              v-for="(token, tokenIndex) in tokenizeCodeLine(line)"
              :key="tokenIndex"
              :class="`token-${token.kind}`"
            >{{ token.text }}</span>
          </code>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.file-preview { @apply h-full min-h-0 flex flex-col overflow-hidden rounded-lg border border-border bg-card; }
.file-preview-toolbar { @apply h-9 shrink-0 flex items-center gap-2 border-b border-border px-2 text-[11px] text-muted; }
.file-preview-toolbar button { @apply rounded p-1 text-muted hover:bg-hover hover:text-foreground; }
.file-preview-toolbar button.is-active { @apply bg-hover text-accent; }
.file-preview-toolbar button :deep(svg) { @apply w-3.5 h-3.5; }
.file-preview-scroll { @apply flex-1 min-h-0 overflow-auto; }
.file-preview-code { @apply min-w-full w-max py-1 font-mono text-xs; }
.file-preview-row { @apply flex min-h-[1.55rem] leading-[1.55rem]; }
.file-preview-line-number { @apply sticky left-0 z-[1] w-12 shrink-0 select-none border-r border-border bg-card px-2 text-right text-[11px] text-muted; }
.file-preview-row code { @apply block px-3 text-foreground whitespace-pre; tab-size: 2; }
.file-preview-code.wrap-lines { @apply w-full min-w-0; }
.file-preview-code.wrap-lines .file-preview-row code { @apply min-w-0 flex-1 whitespace-pre-wrap break-words; }
.token-comment { @apply text-neutral-500 italic; }
.token-string { color: #ce9178; }
.token-number { color: #b5cea8; }
.token-keyword { color: #569cd6; font-weight: 500; }
html.light .token-string { color: #a31515; }
html.light .token-number { color: #098658; }
html.light .token-keyword { color: #0000ff; }
.file-preview-empty { @apply flex-1 flex flex-col items-center justify-center gap-2 p-5 text-center text-xs text-muted; }
.file-preview-empty strong { @apply text-foreground; }
</style>
