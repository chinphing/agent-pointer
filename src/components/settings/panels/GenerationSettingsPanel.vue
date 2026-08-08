<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { Gauge } from 'lucide-vue-next'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const {
  TOOL_CALL_UI_FIELDS,
  displayUiChecked,
  setDisplayUi,
  computerAutoCompact,
  collapseProcessByDefault,
  taskBoardShowChildBoards
} = props.form
</script>

<template>
            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Gauge class="w-4 h-4 text-accent" />界面配置
              </h3>
              <p class="mt-0.5 text-xs text-muted">
                界面显示选项
              </p>
            </div>

            <!-- 工具调用 -->
            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground">工具调用</h4>
              <div class="grid grid-cols-2 gap-y-3 gap-x-32">
                <div
                  v-for="field in TOOL_CALL_UI_FIELDS"
                  :key="field.key"
                  class="flex items-center justify-between gap-3"
                >
                  <h4 class="text-[12px] font-medium text-foreground">{{ field.label }}</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input type="checkbox" class="sr-only peer" :checked="displayUiChecked(field.key)" @change="setDisplayUi(field.key, ($event.target as HTMLInputElement).checked)" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
              </div>
            </div>

            <!-- 智能体输出 -->
            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground">智能体输出</h4>
              <div class="grid grid-cols-2 gap-y-3 gap-x-32">
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">显示推理过程</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showReasoning')" @change="setDisplayUi('showReasoning', ($event.target as HTMLInputElement).checked)" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">显示任务板面板</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showTaskBoardPanel')" @change="setDisplayUi('showTaskBoardPanel', ($event.target as HTMLInputElement).checked)" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">显示子 Agent 边框面板</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showSubAgentTrace')" @change="setDisplayUi('showSubAgentTrace', ($event.target as HTMLInputElement).checked)" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">显示子任务板</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="taskBoardShowChildBoards" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
              </div>
            </div>

            <!-- 执行过程 -->
            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground">执行过程</h4>
              <div class="grid grid-cols-2 gap-y-3 gap-x-32">
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">执行时收缩为状态条</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="computerAutoCompact" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">默认收缩执行过程</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="collapseProcessByDefault" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
              </div>
            </div>
</template>
