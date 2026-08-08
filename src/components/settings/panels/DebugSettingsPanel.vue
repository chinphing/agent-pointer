<script setup lang="ts">
import { Bug } from 'lucide-vue-next'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const { debugDumpLlmPrompts, rawContentViewEnabled, computerAnnotatedScreenViewEnabled } = props.form
</script>

<template>
  <section class="space-y-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
        <Bug class="w-4 h-4 text-accent" />调试
      </h3>
      <p class="mt-0.5 text-xs text-muted">仅用于排查模型请求问题</p>
    </div>

    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">保存每轮对话请求</p>
          <p class="mt-1 text-xs text-muted">将每轮发送给模型的请求保存到本地调试日志</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="debugDumpLlmPrompts" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>

    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">原始内容查看</p>
          <p class="mt-1 text-xs text-muted">助手消息上显示「原始输出」调试入口（正文通道原始字串与 API reasoning）</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="rawContentViewEnabled" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>

    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">标记截图查看</p>
          <p class="mt-1 text-xs text-muted">显示 Computer 使用的标注截图预览</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="computerAnnotatedScreenViewEnabled" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>
  </section>
</template>
