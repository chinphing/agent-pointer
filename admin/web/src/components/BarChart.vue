<script setup lang="ts">
import { computed } from 'vue'
import { fmtCompact, makeTicks } from '../lib/chart'

/**
 * 带刻度的迷你柱状图。
 *
 * 之前两处（概览、我的账户）各写了一份裸柱子：只有相对高度，没有 y 轴，
 * 数值只能靠 hover 的 title 提示，读不出具体量级。这里统一成组件，
 * 左侧刻度 + 底部日期 + 柱顶数值。
 */
export interface BarPoint {
  /** 横轴标签，例如 11-04 */
  label: string
  /** 数值 */
  value: number
  /** 悬停提示，缺省用 label + value 拼 */
  hint?: string
}

const props = withDefaults(
  defineProps<{
    points: BarPoint[]
    /** 绘图区高度（px） */
    height?: number
    /** 柱顶数值是否显示 */
    showValues?: boolean
  }>(),
  { height: 120, showValues: true }
)

const ticks = computed(() => makeTicks(Math.max(0, ...props.points.map((p) => p.value)), 4))

/** 柱高按同一套刻度算，保证柱顶与左侧刻度对得上。 */
const top = computed(() => ticks.value[ticks.value.length - 1]?.value || 1)

/**
 * 柱顶要放数值，所以在**绘图区之外**留一行，而不是把柱子按更小的高度缩放。
 * 之前两者基准不同：刻度按整高、柱子按扣掉标签后的高度，最高的柱子离 1M
 * 刻度线差了一大截，看起来像比例算错了。
 */
const headroom = computed(() => (props.showValues ? 17 : 0))
const plotH = computed(() => Math.max(20, props.height - headroom.value))

function barHeight(v: number): number {
  if (!v || v <= 0) return 3
  return Math.max(4, Math.round((v / top.value) * plotH.value))
}

function hintOf(p: BarPoint): string {
  return p.hint ?? `${p.label}：${fmtCompact(p.value)}`
}
</script>

<template>
  <div class="bc" :style="{ paddingTop: `${headroom}px` }">
    <!-- 左侧刻度：绝对定位到对应高度，与右侧网格线严格对齐 -->
    <div class="bc-axis" :style="{ height: `${plotH}px` }">
      <span
        v-for="(tk, i) in ticks"
        :key="i"
        class="bc-tick"
        :style="{ bottom: `${tk.pct}%` }"
      >
        {{ tk.label }}
      </span>
    </div>

    <div class="bc-plot" :style="{ height: `${plotH}px` }">
      <i
        v-for="(tk, i) in ticks"
        :key="i"
        class="bc-grid"
        :class="{ base: tk.pct === 0 }"
        :style="{ bottom: `${tk.pct}%` }"
      ></i>

      <div class="bc-bars">
        <div v-for="(p, i) in points" :key="i" class="bc-col" :title="hintOf(p)">
          <span v-if="showValues" class="bc-value" :class="{ zero: p.value <= 0 }">
            {{ fmtCompact(p.value) }}
          </span>
          <div
            class="bc-bar"
            :class="{ zero: p.value <= 0 }"
            :style="{ height: `${barHeight(p.value)}px` }"
          ></div>
        </div>
      </div>
    </div>

    <div class="bc-xaxis">
      <span v-for="(p, i) in points" :key="i" class="bc-xlabel">{{ p.label }}</span>
    </div>
  </div>
</template>

<style scoped>
.bc {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  grid-template-rows: auto auto;
  column-gap: 10px;
}

/* 刻度文字右对齐贴着绘图区，避免数字长短不一时左边缘参差 */
/* 刻度文字是绝对定位的，父容器必须有真实宽度，否则宽度塌成 0、
   文字向左溢出到卡片内边距之外（截图里 y 轴贴着卡片边框）。 */
.bc-axis { position: relative; width: 44px; }
.bc-tick {
  position: absolute;
  right: 0;
  transform: translateY(50%);
  font-size: 11px;
  line-height: 1;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.bc-plot { position: relative; }
.bc-grid {
  position: absolute;
  left: 0;
  right: 0;
  height: 0;
  border-top: 1px dashed var(--line);
}
.bc-grid.base { border-top-style: solid; border-color: var(--line); }

.bc-bars {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: flex-end;
  gap: 8px;
}
.bc-col {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  align-items: center;
  gap: 4px;
}
/* 柱体用像素高度，不依赖百分比解析 */
.bc-bar {
  /* 不加 flex:none，内容超出容器时柱子会被压缩，高度就和刻度对不上了 */
  flex: none;
  width: 100%;
  max-width: 46px;
  border-radius: 5px 5px 2px 2px;
  background: var(--accent);
  opacity: 0.85;
}
.bc-bar.zero { background: var(--line); opacity: 1; }
.bc-col:hover .bc-bar { opacity: 1; }

.bc-value.zero { color: var(--muted); }
.bc-value {
  flex: none;
  font-size: 11px;
  line-height: 1;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

/* 横轴与柱状区同列对齐 */
.bc-xaxis {
  grid-column: 2;
  display: flex;
  gap: 8px;
  margin-top: 6px;
}
.bc-xlabel {
  flex: 1;
  min-width: 0;
  text-align: center;
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
