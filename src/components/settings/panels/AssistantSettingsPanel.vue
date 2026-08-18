<script setup lang="ts">
import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { COMPUTER_INITIAL_TIER_OPTIONS } from '../../../types/chat'
import { useSettingsStore } from '../../../stores/settings'
import { Bot, CircleHelp, Code, Film, Gauge, Monitor, SlidersHorizontal, UserRound, X } from 'lucide-vue-next'
import { composerAgentLabel } from '../../../lib/agentUi'
import SceneTierModelsModal from '../SceneTierModelsModal.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()

// 场景三档模型映射弹窗（点场景行「模型」打开）
const sceneModal = ref<string | null>(null)
// 「更多」弹窗：其他执行智能体（explorer 等）的档位与模型设置
const moreModal = ref(false)
const moreWorkers = computed(() =>
  enabledWorkers.value.filter(w => w.id !== 'general' && w.id !== 'coder' && w.id !== 'computer')
)
// 弹窗内短中文描述，风格与左卡「通用助手/氛围编程」一致；未收录的 worker 回退英文原文截断
const WORKER_DESC_ZH: Record<string, string> = {
  explore: '代码库探索、符号映射与资料检索'
}

const mediaModeDesc = (key: string): string => {
  const descs: Record<string, string> = { image: '识别图片内容', audio: '语音转文字', video: '分析视频画面' }
  return descs[key] ?? ''
}

const {
  PERFORMANCE_MODE_HELP,
  MEDIA_MODE_USER_ROWS,
  PERFORMANCE_MODE_UI,
  enabledWorkers,
  computerInitialTier,
  agentPerformanceModesLocal,
  mediaUnderstandingModesLocal,
  userCodingRules
} = props.form

const COMPUTER_TIER_CARDS = COMPUTER_INITIAL_TIER_OPTIONS

</script>

<template>
  <div class="flex-1 flex flex-col gap-5">
    <div class="flex items-start justify-between gap-3 pb-1">
      <div>
        <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
          <Bot class="w-4 h-4 text-accent" />智能体
        </h3>
        <p class="mt-0.5 text-[11px] text-muted">先选择工作方式，再调整执行行为</p>
      </div>
    </div>

    <!-- 场景档位 -->
    <section class="space-y-4" aria-labelledby="assistant-scene-heading">
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
        <div>
          <div class="flex items-center gap-1.5">
            <h4 id="assistant-scene-heading" class="text-sm font-medium text-foreground flex items-center gap-2">
              <Gauge class="w-4 h-4 text-accent" />场景档位
            </h4>
            <button
              type="button"
              class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
              :title="PERFORMANCE_MODE_HELP"
              aria-label="档位说明"
              @click.stop
            >
              <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
            </button>
          </div>
          <p class="mt-1 text-[11px] text-muted">
            每个场景独立选档；点「模型」可调整各档位对应的模型。
          </p>
        </div>
        <div class="grid grid-cols-2 gap-2.5 items-start">
          <div class="rounded-lg border border-border bg-card/50 overflow-hidden">
            <div class="px-3.5 pt-3 pb-2 border-b border-border flex items-center justify-between gap-2">
              <h5 class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">智能体配置</h5>
              <button
                type="button"
                class="inline-flex items-center h-6 px-1.5 rounded-md text-[11px] text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors shrink-0"
                title="调整 explorer 等其他执行智能体的档位与模型"
                aria-label="更多智能体设置"
                @click="moreModal = true"
              >
                更多
              </button>
            </div>
            <div class="divide-y divide-border">
          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Bot class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">通用助手</span>
              <span class="text-[10px] text-muted">日常对话、写作与工具调用</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="'scene-general-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="(agentPerformanceModesLocal.general ?? 'fast') === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    name="scene-general-mode"
                    :checked="(agentPerformanceModesLocal.general ?? 'fast') === opt.value"
                    @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, general: opt.value }"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  title="配置该场景各档位模型"
                  aria-label="配置通用助手档位模型"
                  @click="sceneModal = 'general'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  模型
                </button>
            </div>
          </div>

          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Code class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">氛围编程</span>
              <span class="text-[10px] text-muted">深度重构、跨文件修改与测试</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="'scene-coder-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="(agentPerformanceModesLocal.coder ?? 'fast') === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    name="scene-coder-mode"
                    :checked="(agentPerformanceModesLocal.coder ?? 'fast') === opt.value"
                    @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, coder: opt.value }"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  title="配置该场景各档位模型"
                  aria-label="配置氛围编程档位模型"
                  @click="sceneModal = 'coder'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  模型
                </button>
            </div>
          </div>

          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Monitor class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">电脑操控</span>
              <span class="text-[10px] text-muted">桌面操作、浏览器自动化与文件处理</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in COMPUTER_TIER_CARDS"
                  :key="'scene-desktop-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="computerInitialTier === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    v-model="computerInitialTier"
                    type="radio"
                    class="sr-only"
                    name="scene-desktop-tier"
                    :value="opt.value"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  title="配置该场景各档位模型"
                  aria-label="配置电脑操控档位模型"
                  @click="sceneModal = 'computer'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  模型
                </button>
            </div>
          </div>
            </div>
          </div>

          <div class="rounded-lg border border-border bg-card/50 overflow-hidden">
            <div class="px-3.5 pt-3 pb-2 border-b border-border flex items-center">
              <h5 class="h-6 flex items-center text-[11px] font-semibold uppercase tracking-wider text-muted/80">媒体理解</h5>
            </div>
            <div class="divide-y divide-border">
          <div
            v-for="row in MEDIA_MODE_USER_ROWS"
            :key="'scene-media-' + row.key"
            class="px-3.5 py-3 space-y-2"
          >
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Film class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">{{ row.label }}</span>
              <span class="text-[10px] text-muted">{{ mediaModeDesc(row.key) }}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="row.key + '-mode-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="mediaUnderstandingModesLocal[row.key] === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    :name="'media-mode-' + row.key"
                    :checked="mediaUnderstandingModesLocal[row.key] === opt.value"
                    @change="mediaUnderstandingModesLocal = { ...mediaUnderstandingModesLocal, [row.key]: opt.value }"
                  />
                  {{ opt.label }}
                </label>
              </div>
              <button
                type="button"
                class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                title="配置该场景各档位模型"
                :aria-label="'配置' + row.label + '档位模型'"
                @click="sceneModal = row.key"
              >
                <SlidersHorizontal class="w-3 h-3" />
                模型
              </button>
            </div>
          </div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- 个性化 -->
    <section class="space-y-4" aria-labelledby="assistant-personalize-heading">
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div>
          <h4 id="assistant-personalize-heading" class="text-sm font-medium text-foreground flex items-center gap-2">
            <UserRound class="w-4 h-4 text-accent" />个性化
          </h4>
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
    </section>
    <!-- 场景三档模型映射弹窗 -->
    <SceneTierModelsModal :form="form" :scene="sceneModal ?? ''" :open="sceneModal !== null" @close="sceneModal = null" />

    <!-- 更多智能体弹窗（explorer 等执行智能体的档位与模型） -->
    <Teleport to="body">
      <div
        v-if="moreModal"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="moreModal = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">更多智能体</h4>
              <p class="mt-0.5 text-[11px] text-muted">调整其他执行智能体的档位与模型</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              aria-label="关闭"
              @click="moreModal = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="divide-y divide-border overflow-y-auto">
            <div
              v-for="w in moreWorkers"
              :key="'more-worker-' + w.id"
              class="px-3.5 py-3 space-y-2"
            >
              <div class="flex items-center gap-x-2 min-w-0">
                <Bot class="w-4 h-4 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground shrink-0">{{ composerAgentLabel(w, s.settings) }}</span>
                <span class="text-[10px] text-muted truncate min-w-0">{{ WORKER_DESC_ZH[w.id] ?? w.description }}</span>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                  <label
                    v-for="opt in PERFORMANCE_MODE_UI"
                    :key="'more-' + w.id + '-' + opt.value"
                    class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                    :class="(agentPerformanceModesLocal[w.id] ?? 'fast') === opt.value
                      ? 'bg-hover text-foreground'
                      : 'text-muted hover:text-foreground'"
                  >
                    <input
                      type="radio"
                      class="sr-only"
                      :name="'more-mode-' + w.id"
                      :checked="(agentPerformanceModesLocal[w.id] ?? 'fast') === opt.value"
                      @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, [w.id]: opt.value }"
                    />
                    {{ opt.label }}
                  </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="'配置' + composerAgentLabel(w, s.settings) + '各档位模型'"
                  :aria-label="'配置' + composerAgentLabel(w, s.settings) + '档位模型'"
                  @click="sceneModal = w.id"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  模型
                </button>
              </div>
            </div>
            <p v-if="moreWorkers.length === 0" class="text-xs text-muted text-center py-6">暂无其他执行智能体</p>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
