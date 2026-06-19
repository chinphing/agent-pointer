<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { Bot, CircleHelp, Info, Sparkles, Wrench } from 'lucide-vue-next'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()
const {
  PERFORMANCE_MODE_HELP,
  COMPUTER_INITIAL_TIER_OPTIONS,
  AGENT_MODE_USER_ROWS,
  MEDIA_MODE_USER_ROWS,
  PERFORMANCE_MODE_UI,
  computerInitialTier,
  agentPerformanceModesLocal,
  mediaUnderstandingModesLocal,
  computerAutoCompact,
  userCodingRules,
  computerHumanLike,
  captchaSliderOffsetPx,
  computerAutoSwitchMonitor,
  mediaImageGenerationModel,
  mediaVideoGenerationModel,
  selectMediaModelWithProvider,
  toolApprovalMode,
  contextCompressionEnabled,
  contextBudgetTokens,
  contextKeepRecentUserTurns,
  contextSummaryMaxTokens,
  maxToolRounds,
  mediaDeps,
  ffmpegStatusLabel,
  ffmpegStatusDetail,
  ffmpegNeedsInstall,
  refreshMediaDeps,
  askAssistantInstallFfmpeg
} = props.form
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />智能体
              </h3>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div>
                <div class="flex items-center gap-1.5">
                  <h4 class="text-sm font-medium text-foreground">模式选择</h4>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                    :title="PERFORMANCE_MODE_HELP"
                    aria-label="模式说明"
                    @click.stop
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <p class="mt-1 text-[11px] text-muted">
                  选择各场景的运行模式；具体模型在调试模式中配置。
                </p>
              </div>
              <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 lg:gap-8">
                <div class="space-y-3 min-w-0">
                  <h5 class="text-[12px] font-medium text-foreground flex items-center gap-1.5">
                    <Bot class="w-3.5 h-3.5 text-accent shrink-0" />智能体
                  </h5>
                  <div
                    class="flex flex-wrap items-center gap-x-4 gap-y-2"
                    title="新会话开始时电脑操控使用的模式；会话中仍可能因验证失败自动升级"
                  >
                    <span class="text-[12px] text-foreground whitespace-nowrap shrink-0 w-20">电脑操控</span>
                    <div class="inline-flex flex-wrap items-center gap-3 min-w-0">
                      <label
                        v-for="opt in COMPUTER_INITIAL_TIER_OPTIONS"
                        :key="'computer-tier-' + opt.value"
                        class="inline-flex items-center gap-1.5 cursor-pointer text-[11px] text-muted whitespace-nowrap"
                      >
                        <input
                          type="radio"
                          class="rounded-full border-border bg-card text-accent focus:ring-accent/40"
                          name="computer-initial-tier"
                          :checked="computerInitialTier === opt.value"
                          @change="computerInitialTier = opt.value"
                        />
                        <span class="text-foreground whitespace-nowrap">{{ opt.label }}</span>
                      </label>
                    </div>
                  </div>
                  <div
                    v-for="row in AGENT_MODE_USER_ROWS"
                    :key="'agent-mode-row-' + row.id"
                    class="flex flex-wrap items-center gap-x-4 gap-y-2"
                  >
                    <span class="text-[12px] text-foreground whitespace-nowrap shrink-0 w-20">{{ row.label }}</span>
                    <div class="inline-flex flex-wrap items-center gap-3 min-w-0">
                      <label
                        v-for="opt in PERFORMANCE_MODE_UI"
                        :key="row.id + '-mode-' + opt.value"
                        class="inline-flex items-center gap-1.5 cursor-pointer text-[11px] text-muted whitespace-nowrap"
                      >
                        <input
                          type="radio"
                          class="rounded-full border-border bg-card text-accent focus:ring-accent/40"
                          :name="'agent-mode-' + row.id"
                          :checked="(agentPerformanceModesLocal[row.id] ?? 'fast') === opt.value"
                          @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, [row.id]: opt.value }"
                        />
                        <span class="text-foreground whitespace-nowrap">{{ opt.label }}</span>
                      </label>
                    </div>
                  </div>
                </div>
                <div class="space-y-3 min-w-0 lg:border-l lg:border-border lg:pl-8">
                  <h5 class="text-[12px] font-medium text-foreground flex items-center gap-1.5">
                    <Wrench class="w-3.5 h-3.5 text-accent shrink-0" />工具
                  </h5>
                  <div
                    v-for="row in MEDIA_MODE_USER_ROWS"
                    :key="'media-mode-row-' + row.key"
                    class="flex flex-wrap items-center gap-x-4 gap-y-2"
                  >
                    <span class="text-[12px] text-foreground whitespace-nowrap shrink-0 w-20">{{ row.label }}</span>
                    <div class="inline-flex flex-wrap items-center gap-3 min-w-0">
                      <label
                        v-for="opt in PERFORMANCE_MODE_UI"
                        :key="row.key + '-mode-' + opt.value"
                        class="inline-flex items-center gap-1.5 cursor-pointer text-[11px] text-muted whitespace-nowrap"
                      >
                        <input
                          type="radio"
                          class="rounded-full border-border bg-card text-accent focus:ring-accent/40"
                          :name="'media-mode-' + row.key"
                          :checked="mediaUnderstandingModesLocal[row.key] === opt.value"
                          @change="mediaUnderstandingModesLocal = { ...mediaUnderstandingModesLocal, [row.key]: opt.value }"
                        />
                        <span class="text-foreground whitespace-nowrap">{{ opt.label }}</span>
                      </label>
                    </div>
                  </div>
                </div>
              </div>
              <div class="pt-3 border-t border-border space-y-3">
                <h5 class="text-[12px] font-medium text-foreground">电脑操控选项</h5>
                <div class="grid grid-cols-3 gap-x-8 w-full">
                  <div class="flex items-center gap-1.5 min-w-0">
                    <span class="text-[12px] text-foreground whitespace-nowrap">执行时收缩为状态条</span>
                    <label class="relative inline-flex items-center cursor-pointer shrink-0">
                      <input
                        type="checkbox"
                        class="sr-only peer"
                        :checked="computerAutoCompact"
                        @change="computerAutoCompact = ($event.target as HTMLInputElement).checked"
                      />
                      <div class="settings-toggle-track" />
                    </label>
                  </div>
                  <div class="flex items-center gap-1.5 min-w-0">
                    <span class="text-[12px] text-foreground whitespace-nowrap">人性化鼠标移动</span>
                    <button
                      type="button"
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                      title="启用后鼠标沿曲线移动并带微抖动；关闭时使用直线匀速移动（约 0.5–1.5 秒随机）"
                      aria-label="人性化鼠标移动说明"
                      @click.stop
                    >
                      <Info class="w-3.5 h-3.5 pointer-events-none" />
                    </button>
                    <label class="relative inline-flex items-center cursor-pointer shrink-0">
                      <input
                        type="checkbox"
                        class="sr-only peer"
                        :checked="computerHumanLike"
                        @change="computerHumanLike = ($event.target as HTMLInputElement).checked"
                      />
                      <div class="settings-toggle-track" />
                    </label>
                  </div>
                  <div class="flex items-center gap-1.5 min-w-0">
                    <span class="text-[12px] text-foreground whitespace-nowrap">自动切换屏幕</span>
                    <button
                      type="button"
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                      title="开启：默认主屏，launch_app 后跟随应用窗口；关闭：手动按桌面布局选择屏幕"
                      aria-label="自动切换屏幕说明"
                      @click.stop
                    >
                      <Info class="w-3.5 h-3.5 pointer-events-none" />
                    </button>
                    <label class="relative inline-flex items-center cursor-pointer shrink-0">
                      <input
                        type="checkbox"
                        class="sr-only peer"
                        :checked="computerAutoSwitchMonitor"
                        @change="computerAutoSwitchMonitor = ($event.target as HTMLInputElement).checked"
                      />
                      <div class="settings-toggle-track" />
                    </label>
                  </div>
                  <div class="flex items-center gap-1.5 min-w-0">
                    <span class="text-[12px] text-foreground whitespace-nowrap">滑块终点偏移（px）</span>
                    <input
                      v-model.number="captchaSliderOffsetPx"
                      type="number"
                      step="1"
                      class="w-16 h-8 rounded-lg border border-border bg-card px-2 text-[12px] text-right text-foreground outline-none focus:border-accent/50"
                    />
                  </div>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Sparkles class="w-4 h-4 text-accent" />图片 / 视频生成
              </h4>
              <p class="text-[11px] text-muted">
                暂时支持文本和图片生成视频。
              </p>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">图片生成</label>
                  <select
                    :value="mediaImageGenerationModel"
                    class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
                    @change="selectMediaModelWithProvider('imageGeneration', ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">默认（千问 wan2.7-image-pro 或豆包 Seedream）</option>
                    <option
                      v-for="item in s.imageGenerationModels"
                      :key="'img-gen-' + item.providerId + ':' + item.model"
                      :value="item.providerId + ':' + item.model"
                    >
                      {{ item.providerName }} / {{ item.model }}
                    </option>
                  </select>
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">视频生成</label>
                  <select
                    :value="mediaVideoGenerationModel"
                    class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
                    @change="selectMediaModelWithProvider('videoGeneration', ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">默认（千问 HappyHorse 或豆包 Seedance 2.0）</option>
                    <option
                      v-for="item in s.videoGenerationModels"
                      :key="'vid-gen-' + item.providerId + ':' + item.model"
                      :value="item.providerId + ':' + item.model"
                    >
                      {{ item.providerName }} / {{ item.model }}
                    </option>
                  </select>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Wrench class="w-4 h-4 text-accent" />工具使用权限
              </h4>
              <div class="grid grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'auto' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-foreground">自动执行</span>
                  <span class="mt-1 block text-[11px] text-muted">AI 使用工具时自动执行，无需确认</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'manual' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-foreground">敏感操作确认</span>
                  <span class="mt-1 block text-[11px] text-muted">涉及文件、命令等操作时需要你确认</span>
                </label>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <div>
                <h4 class="text-sm font-medium text-foreground">编码偏好</h4>
                <p class="mt-1 text-[11px] text-muted">
                  写入每次对话的系统提示（[USER RULES]）。用于约束改动范围、风格等；留空则仅使用产品默认规则。
                </p>
              </div>
              <textarea
                v-model="userCodingRules"
                rows="5"
                maxlength="4000"
                placeholder="例如：&#10;- 只改用户明确要求的行为，不顺手重构&#10;- 歧义时先说明假设，不要扩大修复范围&#10;- 相关但未要求的内容放到可选后续，不要一并提交"
                class="w-full min-h-[7rem] px-3 py-2 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors resize-y"
              />
              <p class="text-[10px] text-muted text-right">{{ userCodingRules.length }} / 4000</p>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div class="flex items-center justify-between">
                <h4 class="text-sm font-medium text-foreground">上下文自动压缩</h4>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="contextCompressionEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track"></div>
                </label>
              </div>
              <p class="text-[11px] text-muted">当历史消息超过预算时，自动生成摘要并保留最近若干轮对话原文。</p>

              <div v-if="contextCompressionEnabled" class="grid grid-cols-2 gap-3 pt-2 border-t border-border">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">触发预算（tokens）</label>
                  <input v-model.number="contextBudgetTokens" type="number" min="4096" max="2000000" step="1000" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">保留最近用户轮数</label>
                  <input v-model.number="contextKeepRecentUserTurns" type="number" min="1" max="50" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">摘要最大 tokens</label>
                  <input v-model.number="contextSummaryMaxTokens" type="number" min="128" max="8192" step="64" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">单轮最大工具调用轮次</label>
                  <input v-model.number="maxToolRounds" type="number" min="1" max="10000" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Sparkles class="w-4 h-4 text-accent" />多媒体理解
              </h4>
              <div class="rounded-lg border border-border bg-card/50 px-3 py-2.5 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                <div>
                  <p class="text-[12px] text-foreground">ffmpeg / ffprobe</p>
                  <p class="text-[11px] text-muted">
                    IM 视频与抽帧理解需要本机安装；未安装时不打包进应用。
                  </p>
                  <p
                    class="text-[11px] mt-1"
                    :class="mediaDeps?.status === 'ready' ? 'text-emerald-600' : 'text-amber-600'"
                  >
                    {{ ffmpegStatusLabel }}
                  </p>
                  <p v-if="ffmpegStatusDetail" class="text-[10px] text-muted mt-0.5 break-all">
                    {{ ffmpegStatusDetail }}
                  </p>
                  <p v-if="mediaDeps?.status === 'ready'" class="text-[10px] text-muted mt-0.5">
                    单个视频仍可能因编码或文件损坏抽帧失败，不代表未安装 ffmpeg。
                  </p>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                  <button
                    type="button"
                    class="h-8 px-3 rounded-lg border border-border text-xs text-foreground hover:bg-muted/50"
                    @click="refreshMediaDeps()"
                  >
                    重新检测
                  </button>
                  <button
                    v-if="ffmpegNeedsInstall"
                    type="button"
                    class="h-8 px-3 rounded-lg bg-accent text-accent-foreground text-xs hover:opacity-90"
                    @click="askAssistantInstallFfmpeg()"
                  >
                    让助手安装
                  </button>
                </div>
              </div>
            </div>
</template>
