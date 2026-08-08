/**
 * Window drag policies — single source of truth per UI region.
 * Desktop: only regions with `draggable: true` call `startDragging()` on mousedown.
 * macOS uses `setMovableByWindowBackground(false)` so content never drags the window.
 */

export const WINDOW_DRAG_INTERACTIVE_SELECTOR =
  'button, a, input, textarea, select, [contenteditable="true"], [data-no-window-drag]'

export type WindowDragRegionId =
  | 'collapsed-top-chrome'
  | 'sidebar-top-chrome'
  | 'sidebar-body'
  | 'main-top-chrome'
  | 'settings-top-chrome'
  | 'chat-body'
  | 'compact-bar-shell'
  | 'compact-bar-status'
  | 'compact-bar-actions'

export interface WindowDragRegionPolicy {
  draggable: boolean
  doubleClickMaximize: boolean
}

export const WINDOW_DRAG_REGION_POLICIES: Record<WindowDragRegionId, WindowDragRegionPolicy> = {
  'collapsed-top-chrome': {
    draggable: true,
    doubleClickMaximize: true
  },
  'sidebar-top-chrome': {
    draggable: true,
    doubleClickMaximize: true
  },
  'main-top-chrome': {
    draggable: true,
    doubleClickMaximize: true
  },
  'settings-top-chrome': {
    draggable: true,
    doubleClickMaximize: true
  },
  'sidebar-body': {
    draggable: false,
    doubleClickMaximize: false
  },
  'chat-body': {
    draggable: false,
    doubleClickMaximize: false
  },
  'compact-bar-shell': {
    draggable: true,
    doubleClickMaximize: false
  },
  'compact-bar-status': {
    draggable: false,
    doubleClickMaximize: false
  },
  'compact-bar-actions': {
    draggable: false,
    doubleClickMaximize: false
  }
}
