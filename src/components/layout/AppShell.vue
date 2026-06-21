<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  Plus,
  Search,
  Settings,
  MessageSquare,
  Trash2,
  Bot,
  PanelLeftClose,
  PanelLeftOpen
} from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useWindowChrome } from '../../composables/useWindowChrome'
import { useSidebarCollapse } from '../../composables/useSidebarCollapse'
import WindowControls from './WindowControls.vue'
import WindowDragRegion from './WindowDragRegion.vue'
import DesktopSnapshotButton from './DesktopSnapshotButton.vue'
import { isTauriRuntime } from '../../lib/runtime'

defineEmits<{ (e: 'open-settings'): void }>()

const chat = useChatStore()
const { collapsed: sidebarCollapsed, toggle: toggleSidebar } = useSidebarCollapse()
const {
  enabled: chromeEnabled,
  os,
  maximized,
  showCustomControls,
  macTrafficLightPadding,
  minimize,
  toggleMaximize,
  close: closeWindow
} = useWindowChrome()

/** Windows / Linux: min/max/close on main top-right (or collapsed top strip). */
const useMainAreaWindowControls = computed(
  () => showCustomControls.value && (os.value === 'windows' || os.value === 'linux')
)
const windowControlsOnCollapsedTop = computed(
  () => useMainAreaWindowControls.value && sidebarCollapsed.value
)
const windowControlsOnMainTop = computed(
  () => useMainAreaWindowControls.value && !sidebarCollapsed.value
)

const searchQuery = ref('')

const filteredConversations = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return chat.conversations
  return chat.conversations.filter(c =>
    c.title.toLowerCase().includes(query) ||
    new Date(c.updatedAt).toLocaleString().includes(query)
  )
})
</script>

<template>
  <div class="h-full w-full flex flex-col min-h-0">
    <!-- H: 收起全宽顶栏 -->
    <WindowDragRegion
      v-if="sidebarCollapsed"
      region="collapsed-top-chrome"
      class="collapsed-top-chrome hidden md:flex shrink-0 items-center gap-1 pr-2 select-none bg-transparent"
      :class="chromeEnabled && macTrafficLightPadding
        ? 'traffic-light-inset mac-chrome-row'
        : 'h-10 pl-2'"
    >
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="展开侧栏"
        @click="toggleSidebar"
      >
        <PanelLeftOpen class="w-4 h-4" />
      </button>
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="新建会话"
        @click="chat.newConversation()"
      >
        <Plus class="w-4 h-4" />
      </button>
      <div
        v-if="chromeEnabled"
        class="flex-1 min-w-0 h-full"
      />
      <WindowControls
        v-if="windowControlsOnCollapsedTop"
        class="window-controls-win"
        :maximized="maximized"
        @minimize="minimize"
        @maximize="toggleMaximize"
        @close="closeWindow"
      />
    </WindowDragRegion>

    <div class="flex flex-1 min-h-0">
      <aside
        class="app-sidebar hidden md:flex shrink-0 flex-col border-r border-border bg-card transition-[width] duration-200 ease-out overflow-hidden"
        :class="sidebarCollapsed ? 'w-0 border-r-0' : 'w-[260px]'"
      >
        <!-- A: 侧栏顶栏 -->
        <WindowDragRegion
          v-if="!sidebarCollapsed"
          region="sidebar-top-chrome"
          class="sidebar-chrome shrink-0 flex items-center gap-1 pr-2 select-none bg-transparent"
          :class="chromeEnabled && macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
        >
          <div
            v-if="chromeEnabled"
            class="sidebar-chrome-drag flex-1 min-w-0 h-full"
            :class="macTrafficLightPadding ? 'traffic-light-inset' : 'pl-2'"
          />
          <div
            v-else
            class="flex items-center gap-2 min-w-0 flex-1 pl-2"
          >
            <div class="w-7 h-7 rounded-lg bg-accent/15 border border-border flex items-center justify-center shrink-0">
              <Bot class="w-3.5 h-3.5 text-accent" />
            </div>
            <div class="text-[13px] font-semibold tracking-wide brand-text truncate">Pointer</div>
          </div>

          <button
            type="button"
            class="chrome-icon-btn shrink-0"
            title="收起侧栏"
            @click="toggleSidebar"
          >
            <PanelLeftClose class="w-4 h-4" />
          </button>
        </WindowDragRegion>

        <WindowDragRegion
          v-if="!sidebarCollapsed"
          region="sidebar-body"
          class="flex flex-1 flex-col min-h-0 min-w-0"
        >
          <!-- B: 搜索区 -->
          <div class="px-3 py-2 shrink-0">
            <div class="flex items-center gap-2">
              <div class="flex-1 flex items-center gap-2 h-9 px-3 rounded-lg panel-elevated min-w-0">
                <Search class="w-3.5 h-3.5 text-muted shrink-0" />
                <input
                  v-model="searchQuery"
                  type="text"
                  placeholder="搜索会话"
                  class="flex-1 min-w-0 bg-transparent border-0 outline-none text-xs text-foreground placeholder:text-muted"
                />
              </div>
              <button
                class="h-9 w-9 rounded-lg hover:bg-hover flex items-center justify-center cursor-pointer transition shrink-0"
                @click="chat.newConversation()"
                title="新建会话"
              >
                <Plus class="w-4 h-4 text-muted" />
              </button>
            </div>
          </div>

          <!-- C: 会话列表 -->
          <div class="flex-1 overflow-y-auto px-2 pb-3 space-y-1 min-h-0">
            <div
              v-for="c in filteredConversations"
              :key="c.id"
              class="group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition border"
              :class="c.id === chat.currentId
                ? 'bg-accent-muted border-accent/40'
                : 'hover:bg-hover border-transparent'"
              @click="chat.selectConversation(c.id)"
            >
              <MessageSquare
                class="w-3.5 h-3.5 shrink-0"
                :class="c.id === chat.currentId ? 'text-accent' : 'text-muted'"
              />
              <div class="flex-1 min-w-0">
                <div class="text-[13px] text-foreground truncate">{{ c.title }}</div>
                <div class="text-[10px] text-muted">{{ new Date(c.updatedAt).toLocaleString() }}</div>
              </div>
              <button
                class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-hover cursor-pointer"
                @click.stop="chat.deleteConversation(c.id)"
                title="删除"
              >
                <Trash2 class="w-3.5 h-3.5 text-muted" />
              </button>
            </div>
            <div v-if="!filteredConversations.length" class="px-3 py-8 text-center text-xs text-muted">
              没有找到匹配的会话
            </div>
          </div>

          <!-- F: 侧栏底栏 -->
          <div class="p-2 border-t border-border flex shrink-0 items-center gap-1">
            <DesktopSnapshotButton v-if="!isTauriRuntime()" />
            <button
              class="chrome-icon-btn"
              title="设置"
              @click="$emit('open-settings')"
            >
              <Settings class="w-4 h-4" />
            </button>
          </div>
        </WindowDragRegion>
      </aside>

      <div class="flex-1 min-w-0 flex flex-col">
        <!-- D: 主区顶栏 -->
        <WindowDragRegion
          v-if="chromeEnabled && !sidebarCollapsed"
          region="main-top-chrome"
          class="main-top-chrome hidden md:flex shrink-0 items-stretch select-none min-h-10"
          :class="macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
        >
          <div class="main-chrome-drag flex-1 min-w-0 h-full min-h-10" />
          <WindowControls
            v-if="windowControlsOnMainTop"
            class="window-controls-win"
            :maximized="maximized"
            @minimize="minimize"
            @maximize="toggleMaximize"
            @close="closeWindow"
          />
        </WindowDragRegion>

        <!-- E: 聊天正文 -->
        <WindowDragRegion region="chat-body" as="main" class="flex-1 min-h-0 flex flex-col">
          <slot />
        </WindowDragRegion>
      </div>
    </div>
  </div>
</template>
