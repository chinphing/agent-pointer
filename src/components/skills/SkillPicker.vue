<script setup lang="ts">
import { computed, ref } from 'vue'
import { X, Sparkles, Search, Wrench, Upload } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'

defineEmits<{ (e: 'close'): void }>()
const skills = useSkillsStore()
const q = ref('')
const importing = ref(false)
const importMessage = ref('')
const fileInput = ref<HTMLInputElement | null>(null)

const filtered = computed(() => {
  const k = q.value.trim().toLowerCase()
  if (!k) return skills.skills
  return skills.skills.filter(s =>
    s.name.toLowerCase().includes(k) ||
    s.description.toLowerCase().includes(k) ||
    s.tags.join(' ').toLowerCase().includes(k)
  )
})

async function onImportFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  importing.value = true
  importMessage.value = ''
  try {
    const result = await skills.importZip(file)
    const names = result.imported.map(s => s.name).join('、')
    importMessage.value = result.imported.length
      ? `已导入 ${result.imported.length} 个技能：${names}`
      : '未导入任何技能'
    if (result.skipped.length) {
      importMessage.value += `；跳过 ${result.skipped.length} 项`
    }
  } catch (err: any) {
    importMessage.value = String(err?.message || err)
  } finally {
    importing.value = false
    input.value = ''
  }
}
</script>


<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="$emit('close')">
    <div class="w-[720px] max-w-[92vw] max-h-[80vh] glass-strong rounded-2xl border border-white/10 shadow-2xl flex flex-col overflow-hidden">
      <header class="px-5 h-14 flex items-center gap-2 border-b border-white/5">
        <Sparkles class="w-4 h-4 text-primary-fuchsia" />
        <h2 class="text-base font-semibold text-slate-100">Skills 技能库</h2>
        <span class="text-xs text-slate-500">启用后先注入 frontmatter 索引，正文与资源按需加载</span>
        <div class="flex-1" />
        <input ref="fileInput" type="file" accept=".zip,application/zip" class="hidden" @change="onImportFile" />
        <button
          class="h-8 px-3 rounded-lg glass hover:bg-white/10 cursor-pointer text-xs text-slate-200 flex items-center gap-1.5 disabled:opacity-50"
          :disabled="importing"
          @click="fileInput?.click()"
        >
          <Upload class="w-3.5 h-3.5 text-primary-cyan" />
          {{ importing ? '导入中...' : '导入 zip' }}
        </button>
        <button class="p-2 rounded-lg hover:bg-white/5 cursor-pointer" @click="$emit('close')">

          <X class="w-4 h-4 text-slate-300" />
        </button>
      </header>

      <div class="px-5 pt-3">
        <div v-if="importMessage" class="mb-3 rounded-xl border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-slate-300">
          {{ importMessage }}
        </div>
        <div class="flex items-center gap-2 h-10 px-3 rounded-xl glass">

          <Search class="w-4 h-4 text-slate-400" />
          <input
            v-model="q"
            type="text"
            placeholder="搜索技能名、说明或标签"
            class="flex-1 bg-transparent border-0 outline-none text-sm text-slate-100 placeholder:text-slate-500"
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-5 grid grid-cols-1 md:grid-cols-2 gap-3">
        <div
          v-for="s in filtered"
          :key="s.id"
          class="rounded-xl p-4 border transition cursor-pointer"
          :class="skills.isEnabled(s.id)
            ? 'border-primary/40 bg-primary/10 shadow-lg shadow-primary/10'
            : 'border-white/5 glass hover:bg-white/[0.06]'"
          @click="skills.toggle(s.id)"
        >
          <div class="flex items-center gap-2">
            <div class="text-[15px] font-semibold text-slate-100">{{ s.name }}</div>
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-white/5 text-slate-400">{{ s.builtin ? '内置' : '外部' }}</span>
            <span v-if="skills.isEnabled(s.id)" class="ml-auto text-[10px] px-2 py-0.5 rounded-full bg-primary/30 text-primary-cyan">已启用</span>

            <span v-else class="ml-auto text-[10px] px-2 py-0.5 rounded-full bg-white/5 text-slate-400">未启用</span>
          </div>
          <p class="mt-1.5 text-[12px] text-slate-400 leading-5 line-clamp-3">{{ s.description }}</p>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <span v-for="t in s.tags" :key="t" class="text-[10px] px-1.5 py-0.5 rounded bg-white/5 text-slate-300">{{ t }}</span>
          </div>
          <div v-if="s.toolNames.length" class="mt-2 flex items-center gap-1 text-[11px] text-slate-400">
            <Wrench class="w-3 h-3 text-primary-fuchsia" />
            <span>{{ s.toolNames.join(' · ') }}</span>
          </div>
          <div v-if="s.resourceFiles.length" class="mt-1 text-[11px] text-slate-500">
            资源文件：{{ s.resourceFiles.length }} 个，按需读取
          </div>
        </div>
        <div v-if="!filtered.length" class="col-span-full text-center text-slate-500 py-12 text-sm">没有匹配的技能</div>
      </div>
    </div>
  </div>
</template>
