<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { JsonPreviewMatch, JsonPreviewNode } from '../../lib/workspaceJsonPreview'
import {
  isJsonArrayItemId,
  isJsonContainer,
  jsonCollapsedSummary,
  jsonPreviewSearchParts,
  jsonTokenKind
} from '../../lib/workspaceJsonPreview'

defineOptions({ name: 'WorkspaceJsonTree' })

const { t } = useI18n()

const props = defineProps<{
  node: JsonPreviewNode
  depth?: number
  collapsed: Set<string>
  matches: JsonPreviewMatch[]
  searchOpen: boolean
  activeMatchIndex: number
}>()

const emit = defineEmits<{
  (e: 'toggle', nodeId: string): void
}>()

const depth = props.depth ?? 0

function hasChildren(node: JsonPreviewNode): boolean {
  return isJsonContainer(node) && node.children.length > 0
}

function expanded(node: JsonPreviewNode): boolean {
  return !props.collapsed.has(node.id)
}

function showKey(node: JsonPreviewNode): boolean {
  return node.key != null && !isJsonArrayItemId(node.id)
}

function keyParts(node: JsonPreviewNode) {
  const text = node.key ?? ''
  if (!text) return []
  if (!props.searchOpen) return [{ text }]
  return jsonPreviewSearchParts(text, node.id, 'key', props.matches)
}

function valueParts(node: JsonPreviewNode) {
  const text = isJsonContainer(node)
    ? (expanded(node) ? '' : jsonCollapsedSummary(node))
    : node.text
  if (!text) return []
  if (!props.searchOpen) return [{ text }]
  return jsonPreviewSearchParts(text, node.id, 'value', props.matches)
}

function toggle(nodeId: string) {
  emit('toggle', nodeId)
}
</script>

<template>
  <div class="json-tree">
    <div
      class="json-row"
      :style="{ paddingLeft: `${8 + depth * 16}px` }"
      :data-json-node-id="node.id"
    >
      <button
        v-if="hasChildren(node)"
        type="button"
        class="json-toggle"
        :aria-expanded="expanded(node)"
        :aria-label="expanded(node) ? t('workspace.jsonTree.collapse') : t('workspace.jsonTree.expand')"
        @click="toggle(node.id)"
      >
        <ChevronDown v-if="expanded(node)" />
        <ChevronRight v-else />
      </button>
      <span v-else class="json-toggle-spacer" />
      <template v-if="showKey(node)">
        <span class="token-string">
          <template
            v-for="(part, partIndex) in keyParts(node)"
            :key="`k-${partIndex}`"
          >
            <mark
              v-if="part.matchIndex != null"
              class="file-preview-search-mark"
              :class="part.matchIndex === activeMatchIndex && 'is-active-match'"
              :data-file-search-match="part.matchIndex"
            >{{ part.text }}</mark>
            <template v-else>{{ part.text }}</template>
          </template>
        </span>
        <span class="json-colon">:</span>
      </template>
      <button
        v-if="hasChildren(node)"
        type="button"
        class="json-bracket"
        :class="!expanded(node) && 'is-collapsed'"
        @click="toggle(node.id)"
      >
        <template v-if="expanded(node)">{{ node.kind === 'array' ? '[' : '{' }}</template>
        <template
          v-else
          v-for="(part, partIndex) in valueParts(node)"
          :key="`s-${partIndex}`"
        >
          <mark
            v-if="part.matchIndex != null"
            class="file-preview-search-mark"
            :class="part.matchIndex === activeMatchIndex && 'is-active-match'"
            :data-file-search-match="part.matchIndex"
          >{{ part.text }}</mark>
          <template v-else>{{ part.text }}</template>
        </template>
      </button>
      <span
        v-else-if="isJsonContainer(node)"
        class="json-bracket"
      >
        {{ node.kind === 'array' ? '[]' : '{}' }}
      </span>
      <span
        v-else
        :class="`token-${jsonTokenKind(node.kind)}`"
      >
        <template
          v-for="(part, partIndex) in valueParts(node)"
          :key="`v-${partIndex}`"
        >
          <mark
            v-if="part.matchIndex != null"
            class="file-preview-search-mark"
            :class="part.matchIndex === activeMatchIndex && 'is-active-match'"
            :data-file-search-match="part.matchIndex"
          >{{ part.text }}</mark>
          <template v-else>{{ part.text }}</template>
        </template>
      </span>
    </div>
    <template v-if="hasChildren(node) && expanded(node)">
      <WorkspaceJsonTree
        v-for="child in node.children"
        :key="child.id"
        :node="child"
        :depth="depth + 1"
        :collapsed="collapsed"
        :matches="matches"
        :search-open="searchOpen"
        :active-match-index="activeMatchIndex"
        @toggle="toggle"
      />
      <div
        class="json-row json-close"
        :style="{ paddingLeft: `${8 + depth * 16}px` }"
      >
        <span class="json-toggle-spacer" />
        <span class="json-bracket">{{ node.kind === 'array' ? ']' : '}' }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.json-row { @apply flex min-h-[1.55rem] items-center gap-1 font-mono text-xs leading-[1.55rem]; }
.json-toggle {
  @apply flex h-4 w-4 shrink-0 items-center justify-center rounded text-muted hover:bg-hover hover:text-foreground;
}
.json-toggle :deep(svg) { @apply h-3 w-3; }
.json-toggle-spacer { @apply h-4 w-4 shrink-0; }
.json-colon { @apply text-muted; }
.json-bracket { @apply text-muted; }
.json-bracket.is-collapsed { @apply cursor-pointer rounded px-0.5 hover:bg-hover hover:text-foreground; }
.json-bracket:disabled { @apply cursor-default; }
.token-string { color: #ce9178; }
.token-number { color: #b5cea8; }
.token-keyword { color: #569cd6; font-weight: 500; }
html.light .token-string { color: #a31515; }
html.light .token-number { color: #098658; }
html.light .token-keyword { color: #0000ff; }
.file-preview-search-mark {
  color: inherit;
  background: hsl(var(--search-mark) / 0.5);
  border-radius: 0.2rem;
  box-shadow: 0 0 0 1px hsl(var(--search-mark) / 0.28);
  padding: 0 0.08em;
}
.file-preview-search-mark.is-active-match {
  background: hsl(var(--search-mark) / 0.72);
  box-shadow: 0 0 0 1px hsl(var(--warning) / 0.55);
}
</style>
