import {
  AppWindow,
  Clipboard,
  Clock,
  FilePen,
  FilePlus,
  FileSearch,
  FileText,
  FolderOpen,
  GitMerge,
  Globe,
  Image as ImageIcon,
  Keyboard,
  LayoutGrid,
  Link,
  ListTodo,
  MessageCircleQuestion,
  MousePointer2,
  ScanEye,
  Search,
  ShieldCheck,
  Sparkles,
  SquareTerminal,
  Timer,
  Video,
  Wrench,
  type LucideIcon
} from 'lucide-vue-next'
import { toolCallBaseName } from './messageTooling'

/**
 * Lucide outline icons for tool rows (same stroke as chevrons).
 * Unknown tools fall back to Wrench and keep their text label.
 */
export function toolCallKindIcon(name: string): LucideIcon {
  const base = toolCallBaseName(name.trim())
  if (base === 'terminal') return SquareTerminal
  if (base === 'file_read') return FileText
  if (base === 'file_write') return FilePlus
  if (base === 'file_edit') return FilePen
  if (
    base === 'file_grep'
    || base === 'file_glob'
    || base === 'session_search'
    || base === 'memory'
  ) {
    return Search
  }
  if (base === 'file_list') return FolderOpen
  if (base === 'web_search') return Globe
  if (base === 'web_fetch') return Link
  if (base.startsWith('skill_')) return Sparkles
  if (base === 'run_subagent') return GitMerge
  if (base === 'ask_user') return MessageCircleQuestion
  if (base === 'cron_job') return Clock
  if (base === 'mouse' || base.startsWith('mouse_') || base.startsWith('modified_click_')) {
    return MousePointer2
  }
  if (base.startsWith('input_') || base === 'hotkey' || base === 'keyboard') return Keyboard
  if (base === 'wait') return Timer
  if (base === 'clipboard' || base.startsWith('clipboard_')) return Clipboard
  if (base === 'launch_app') return AppWindow
  if (base === 'list_apps') return LayoutGrid
  if (base === 'image_generate') return ImageIcon
  if (base === 'video_generate') return Video
  if (base === 'media_understand') return ScanEye
  if (base.startsWith('task_board')) return ListTodo
  if (base.startsWith('captcha_verify')) return ShieldCheck
  if (base === 'read_lints') return FileSearch
  return Wrench
}

/** True when the kind is in the icon map (not the unknown-tool wrench). */
export function isKnownToolKind(name: string): boolean {
  return toolCallKindIcon(name) !== Wrench
}

/**
 * Keep the kind name for 询问用户 (no summary) and unknown tools.
 * Other kinds, including 委派, replace the name with the icon.
 */
export function toolCallShowsKindLabel(name: string): boolean {
  const base = toolCallBaseName(name.trim())
  if (base === 'ask_user') return true
  return !isKnownToolKind(name)
}
