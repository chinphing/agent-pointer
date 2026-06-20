<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { Bot, Sparkles, Users } from 'lucide-vue-next'
import { composerAgentLabel } from '../../../lib/agentUi'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()
const {
  enabledWorkers,
  isModeAgent,
  showDebugMenus,
  platformReadOnly,
  getAgentModelWithProvider,
  selectAgentModelWithProvider,
  taskBoardTrimChecked,
  setTaskBoardTrimLocal,
  PERFORMANCE_MODE_UI,
  agentModeLlm,
  selectAgentModeModel,
  patchAgentModeLlm,
  COMPUTER_TIER_UI,
  computerTierLlm,
  patchComputerTierLlm,
  qwenModelOptions,
  MEDIA_DEBUG_KINDS,
  mediaModeLlm,
  selectMediaModeModel,
  patchMediaModeLlm,
  agentMode,
  TEAM_MODE_UI_ENABLED,
  supervisorAgent,
  maxSubAgentToolRounds,
  maxSubAgentSpawnDepth,
  isLeadWorkerSelected,
  isLeadAgentSelectable,
  selectLeadWorker
} = props.form

function mediaDebugModelOptions(kind: (typeof MEDIA_DEBUG_KINDS)[number]) {
  if (kind === 'audio') return s.audioModels
  return s.visionModels
}
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />智能模式
              </h3>
              <p class="mt-0.5 text-xs text-muted">选择 AI 的工作方式和工具使用权限</p>
            </div>

            <!-- Agent Cards -->
            <div class="space-y-2">
              <h4 class="text-[12px] font-medium text-muted uppercase tracking-wider">执行智能体</h4>

              <!-- Worker Agents -->
              <div
                v-for="w in enabledWorkers"
                :key="w.id"
                class="rounded-xl border p-3 transition-all"
                :class="[
                  isLeadWorkerSelected(w.id) ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))]',
                  isLeadAgentSelectable(w) ? 'cursor-pointer hover:border-border' : ''
                ]"
                @click="selectLeadWorker(w)"
              >
                <div class="flex items-start gap-3">
                  <!-- Icon -->
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="isLeadWorkerSelected(w.id) ? 'bg-accent/10' : 'bg-[hsl(var(--card-elevated))]'">
                    <Bot class="w-4 h-4" :class="isLeadWorkerSelected(w.id) ? 'text-accent' : 'text-muted'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ composerAgentLabel(w, s.settings) }}</span>
                      <span class="px-1.5 py-0.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[10px] text-muted font-mono">{{ w.name }}</span>
                      <span v-if="!isLeadAgentSelectable(w)" class="px-1.5 py-0.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[10px] text-muted">子智能体</span>
                      <span v-else-if="isLeadWorkerSelected(w.id)" class="px-1.5 py-0.5 rounded bg-accent/15 text-[10px] font-medium text-accent">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted">{{ w.description || '通用智能体' }}</p>

                    <!-- Per-agent default model (lead or delegated sub-agent runs) -->
                    <div class="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-2" @click.stop>
                      <div v-if="!isModeAgent(w.id)" class="flex items-center gap-2 min-w-0">
                        <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                        <span class="text-[11px] text-muted shrink-0">默认模型</span>
                        <select
                          :value="getAgentModelWithProvider(w.id)"
                          @change.stop="selectAgentModelWithProvider(w.id, ($event.target as HTMLSelectElement).value)"
                          @click.stop
                          class="w-48 h-7 px-2 rounded bg-card border border-border text-[11px] text-foreground cursor-pointer outline-none focus:border-accent/50 transition-colors"
                        >
                          <option value="">使用全局默认</option>
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                      </div>

                      <label class="inline-flex items-center gap-1.5 cursor-pointer shrink-0">
                        <input
                          type="checkbox"
                          class="rounded border-border bg-card text-accent focus:ring-accent/40"
                          :checked="taskBoardTrimChecked(w.id)"
                          @change="setTaskBoardTrimLocal(w.id, ($event.target as HTMLInputElement).checked)"
                        />
                        <span class="text-[11px] text-muted">任务板后精简历史</span>
                      </label>

                    </div>

                    <div
                      v-if="isModeAgent(w.id) && showDebugMenus"
                      class="col-span-full mt-3 rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <h4 class="text-xs font-medium text-foreground">各模式对应模型（调试）</h4>
                        <span class="text-[10px] text-muted">快速 / 标准 / 专家 各模式对应模型</span>
                      </div>
                      <div
                        v-for="mode in PERFORMANCE_MODE_UI"
                        :key="w.id + '-tier-' + mode.value"
                        class="grid grid-cols-[3rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                      >
                        <span class="text-[11px] text-muted font-medium">{{ mode.label }}</span>
                        <select
                          :value="agentModeLlm(w.id, mode.value).model"
                          class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          @change="selectAgentModeModel(w.id, mode.value, ($event.target as HTMLSelectElement).value)"
                        >
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                        <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                          <input
                            type="checkbox"
                            class="rounded border-border bg-[hsl(var(--card-elevated))]"
                            :checked="agentModeLlm(w.id, mode.value).enableThinking !== false"
                            @change="patchAgentModeLlm(w.id, mode.value, { enableThinking: ($event.target as HTMLInputElement).checked })"
                          />
                          思考
                        </label>
                        <input
                          type="number"
                          min="256"
                          step="256"
                          class="h-8 w-full px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          :value="agentModeLlm(w.id, mode.value).thinkingBudget ?? 2048"
                          :disabled="agentModeLlm(w.id, mode.value).enableThinking === false"
                          @change="patchAgentModeLlm(w.id, mode.value, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                        />
                      </div>
                    </div>

                    <div
                      v-if="w.id === 'computer'"
                      class="col-span-full mt-3 rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <h4 class="text-xs font-medium text-foreground">电脑操控各模式对应模型（调试）</h4>
                        <span class="text-[10px] text-muted">快速 / 标准 / 专家 各模式对应模型与思考参数</span>
                      </div>
                      <div
                        v-for="tier in COMPUTER_TIER_UI"
                        :key="tier.key"
                        class="grid grid-cols-[4.5rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                      >
                        <span class="text-[11px] text-muted font-medium">{{ tier.label }}</span>
                        <select
                          :value="computerTierLlm(tier.key).model"
                          class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          @change="patchComputerTierLlm(tier.key, { model: ($event.target as HTMLSelectElement).value })"
                        >
                          <option v-for="m in qwenModelOptions" :key="m" :value="m">{{ m }}</option>
                        </select>
                        <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                          <input
                            type="checkbox"
                            class="rounded border-border bg-[hsl(var(--card-elevated))]"
                            :checked="computerTierLlm(tier.key).enableThinking !== false"
                            @change="patchComputerTierLlm(tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })"
                          />
                          思考
                        </label>
                        <input
                          type="number"
                          min="256"
                          step="256"
                          class="h-8 w-full px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          :value="computerTierLlm(tier.key).thinkingBudget ?? 2048"
                          :disabled="computerTierLlm(tier.key).enableThinking === false"
                          @change="patchComputerTierLlm(tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                        />
                      </div>
                    </div>

                  </div>
                </div>
              </div>

              <div
                v-if="showDebugMenus"
                class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-2">
                  <h4 class="text-xs font-medium text-foreground">多媒体理解各模式对应模型（调试）</h4>
                  <span class="text-[10px] text-muted">图片 / 语音 / 视频各模式对应模型</span>
                </div>
                <div v-for="kind in MEDIA_DEBUG_KINDS" :key="'media-debug-' + kind" class="space-y-2">
                  <span class="text-[12px] text-foreground font-medium">{{ kind === 'image' ? '图片' : kind === 'audio' ? '语音' : '视频' }}</span>
                  <div
                    v-for="mode in PERFORMANCE_MODE_UI"
                    :key="kind + '-' + mode.value"
                    class="grid grid-cols-[3rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                  >
                    <span class="text-[11px] text-muted font-medium">{{ mode.label }}</span>
                    <select
                      :value="mediaModeLlm(kind, mode.value).model"
                      class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                      @change="selectMediaModeModel(kind, mode.value, ($event.target as HTMLSelectElement).value)"
                    >
                      <option v-for="item in mediaDebugModelOptions(kind)" :key="item.providerId + ':' + item.model" :value="item.model">{{ item.providerName }} / {{ item.model }}</option>
                    </select>
                    <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                      <input
                        type="checkbox"
                        class="rounded border-border bg-[hsl(var(--card-elevated))]"
                        :checked="mediaModeLlm(kind, mode.value).enableThinking !== false"
                        @change="patchMediaModeLlm(kind, mode.value, { enableThinking: ($event.target as HTMLInputElement).checked })"
                      />
                      思考
                    </label>
                    <input
                      type="number"
                      min="256"
                      step="256"
                      class="h-8 w-full px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                      :value="mediaModeLlm(kind, mode.value).thinkingBudget ?? 2048"
                      :disabled="mediaModeLlm(kind, mode.value).enableThinking === false"
                      @change="patchMediaModeLlm(kind, mode.value, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                    />
                  </div>
                </div>
              </div>

              <!-- 团队模式 -->
              <div
                v-if="supervisorAgent"
                class="rounded-xl border p-3 cursor-pointer transition-all"
                :class="agentMode === 'supervisor' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'"
                @click="agentMode = 'supervisor'"
              >
                <div class="flex items-start gap-3">
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="agentMode === 'supervisor' ? 'bg-accent/10' : 'bg-[hsl(var(--card-elevated))]'">
                    <Users class="w-4 h-4" :class="agentMode === 'supervisor' ? 'text-accent' : 'text-muted'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ composerAgentLabel(supervisorAgent, s.settings) }}</span>
                      <span v-if="agentMode === 'supervisor'" class="px-1.5 py-0.5 rounded bg-accent/15 text-[10px] font-medium text-accent">已选择</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted">多子智能体编排与结果整合</p>

                    <!-- Default Model Selector -->
                    <div class="mt-2.5 flex items-center gap-2" @click.stop>
                      <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                      <span class="text-[11px] text-muted shrink-0">默认模型</span>
                      <select
                        :value="getAgentModelWithProvider('supervisor')"
                        @change.stop="selectAgentModelWithProvider('supervisor', ($event.target as HTMLSelectElement).value)"
                        @click.stop
                        class="w-48 h-7 px-2 rounded bg-card border border-border text-[11px] text-foreground cursor-pointer outline-none focus:border-accent/50 transition-colors"
                      >
                        <option value="">使用全局默认</option>
                        <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div
              v-if="agentMode === 'single'"
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3"
            >
              <h4 class="text-sm font-medium text-foreground">子任务委托</h4>
              <p class="text-[11px] text-muted">
                可委派的 worker 由主 Agent 的 AGENT.md 中 <code class="text-muted">allowAgents</code> 配置。
              </p>
              <div>
                <label class="block text-[12px] text-muted mb-1.5">子 Agent 内工具轮次上限</label>
                <input
                  v-model.number="maxSubAgentToolRounds"
                  type="number"
                  min="1"
                  max="10000"
                  step="1"
                  class="w-full max-w-xs h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <label class="block text-[12px] text-muted mb-1.5">子 Agent 最大嵌套深度</label>
                <p class="text-[11px] text-muted mb-1.5">1 = 仅主 agent 可委派；2 = 子 agent 可再委派一层（默认）。</p>
                <input
                  v-model.number="maxSubAgentSpawnDepth"
                  type="number"
                  min="1"
                  max="8"
                  step="1"
                  class="w-full max-w-xs h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
            </div>


</template>
