<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { AlertCircle, Gauge, Plus, Trash2 } from 'lucide-vue-next'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const {
  TOOL_CALL_UI_FIELDS,
  displayUiChecked,
  setDisplayUi,
  rawContentViewEnabled,
  computerAnnotatedScreenViewEnabled,
  taskBoardShowChildBoards,
  debugDumpLlmPrompts,
  terminalEnvRows,
  addTerminalEnvRow,
  removeTerminalEnvRow
} = props.form

const TERMINAL_ENV_HINT =
  '覆盖进程、.env 与会话注入到 terminal 的变量（含 WORKING_DIR、SESSION_USER_ID）；PATH 前置合并。'
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

            <!-- 终端环境变量（会话级；关闭调试后仍注入） -->
            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0">
                  <div class="flex items-center gap-1.5">
                    <h4 class="text-sm font-medium text-foreground">终端环境变量</h4>
                    <span
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help"
                      :title="TERMINAL_ENV_HINT"
                    >
                      <AlertCircle class="w-3.5 h-3.5 pointer-events-none" />
                    </span>
                  </div>
                  <p class="mt-0.5 text-xs text-muted">覆盖注入到终端；仅本次会话，关闭调试后仍生效</p>
                </div>
                <button
                  v-if="terminalEnvRows.length > 0"
                  type="button"
                  class="inline-flex h-8 shrink-0 items-center gap-1 rounded-md border border-border bg-card px-2.5 text-[12px] text-foreground hover:bg-hover cursor-pointer"
                  @click="addTerminalEnvRow"
                >
                  <Plus class="w-3.5 h-3.5" />
                  添加
                </button>
              </div>

              <button
                v-if="terminalEnvRows.length === 0"
                type="button"
                class="flex w-full flex-col items-center justify-center gap-1 rounded-lg border border-dashed border-border bg-card/40 px-4 py-6 text-muted hover:border-border hover:bg-hover/40 hover:text-foreground transition-colors cursor-pointer"
                @click="addTerminalEnvRow"
              >
                <Plus class="w-4 h-4" />
                <span class="text-[12px]">添加变量</span>
              </button>

              <div v-else class="space-y-2">
                <div class="flex items-center gap-2 px-0.5">
                  <span class="w-[10.5rem] shrink-0 text-[11px] text-muted">名称</span>
                  <span class="min-w-0 flex-1 text-[11px] text-muted">值</span>
                  <span class="w-8 shrink-0" aria-hidden="true" />
                </div>
                <div
                  v-for="row in terminalEnvRows"
                  :key="row.id"
                  class="flex items-center gap-2"
                >
                  <input
                    v-model="row.key"
                    type="text"
                    placeholder="例如 WORKING_DIR"
                    class="input-base w-[10.5rem] shrink-0 font-mono text-[12px]"
                    spellcheck="false"
                    autocomplete="off"
                  />
                  <input
                    v-model="row.value"
                    type="text"
                    placeholder="变量值"
                    class="input-base min-w-0 flex-1 font-mono text-[12px]"
                    spellcheck="false"
                    autocomplete="off"
                  />
                  <button
                    type="button"
                    class="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-danger cursor-pointer"
                    title="删除"
                    @click="removeTerminalEnvRow(row.id)"
                  >
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
            </div>
</template>
