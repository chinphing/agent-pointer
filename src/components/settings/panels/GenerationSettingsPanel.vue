<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { Gauge } from 'lucide-vue-next'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()
const {
  TOOL_CALL_UI_FIELDS,
  displayUiChecked,
  setDisplayUi,
  rawContentViewEnabled,
  computerAnnotatedScreenViewEnabled,
  taskBoardShowChildBoards,
  taskBoardWorkItemsEnabled,
  taskBoardPlannerEnabled,
  taskBoardComputerNoExecInit,
  debugDumpLlmPrompts
} = props.form
</script>

<template>            <div>
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
                  <h4 class="text-[12px] font-medium text-foreground">原始内容查看</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="rawContentViewEnabled" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">标记截图查看</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="computerAnnotatedScreenViewEnabled" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
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
                <div class="flex items-center justify-between gap-3">
                  <h4 class="text-[12px] font-medium text-foreground">保存每轮对话请求</h4>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="debugDumpLlmPrompts" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
              </div>
            </div>

            <!-- 任务板 -->
            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground">任务板</h4>
              <div class="grid grid-cols-2 gap-y-3 gap-x-32">
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <h4 class="text-[12px] font-medium text-foreground">启用工作项队列</h4>
                    <p class="text-[11px] text-muted mt-0.5">外部 work_items 批量任务与面板批次列表</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="taskBoardWorkItemsEnabled" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <h4 class="text-[12px] font-medium text-foreground">启用任务板规划器</h4>
                    <p class="text-[11px] text-muted mt-0.5">Computer 每轮用户消息前先运行规划循环</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input v-model="taskBoardPlannerEnabled" type="checkbox" class="sr-only peer" />
                    <div class="settings-toggle-track" />
                  </label>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <h4 class="text-[12px] font-medium text-foreground">规划器模式下禁执行 init</h4>
                    <p class="text-[11px] text-muted mt-0.5">开启规划器后，Computer 执行层不可调用 task_board_init/replace</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0">
                    <input
                      v-model="taskBoardComputerNoExecInit"
                      type="checkbox"
                      class="sr-only peer"
                      :disabled="!taskBoardPlannerEnabled"
                    />
                    <div class="settings-toggle-track" :class="{ 'opacity-40': !taskBoardPlannerEnabled }" />
                  </label>
                </div>
              </div>
            </div>

</template>
