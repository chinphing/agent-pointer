<script setup lang="ts">
import { useSettingsStore } from '../../../stores/settings'
import { Database, Network } from 'lucide-vue-next'

const s = useSettingsStore()
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Database class="w-4 h-4 text-accent" />运行时与存储
              </h3>
              <p class="mt-0.5 text-xs text-muted">查看当前存储与网络运行方式</p>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div class="rounded-xl border border-border panel p-5">
                <div class="w-9 h-9 rounded-lg bg-accent/10 flex items-center justify-center mb-3">
                  <Database class="w-4 h-4 text-accent" />
                </div>
                <div class="text-sm font-medium text-foreground">本地数据</div>
                <p class="mt-1 text-xs text-muted">平台账户凭据保存在 auth.dat；智能体配置保存在 local_platform_settings.json；其余配置仅在本次会话有效。</p>
              </div>
              <div class="rounded-xl border border-border panel p-5">
                <div class="w-9 h-9 rounded-lg bg-accent/10 flex items-center justify-center mb-3">
                  <Network class="w-4 h-4 text-accent" />
                </div>
                <div class="text-sm font-medium text-foreground">网络</div>
                <p class="mt-1 text-xs text-muted">当前直接访问 AI 服务 API。</p>
              </div>
            </div>

</template>
