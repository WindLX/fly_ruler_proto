<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  CirclePause,
  CirclePlay,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Radio,
  RefreshCw,
} from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import { formatAbsoluteTime, formatRelativeTime } from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const { t, locale } = useI18n()
const speed = ref(1)
const busy = ref(false)
const playback = computed(() => server.playback)

async function run(action: () => Promise<void>) {
  busy.value = true
  try {
    await action()
  } finally {
    busy.value = false
  }
}

function toggleTheme() {
  workspace.workspace.theme = workspace.workspace.theme === 'dark' ? 'light' : 'dark'
}

function toggleLocale() {
  locale.value = locale.value === 'zh-CN' ? 'en' : 'zh-CN'
  workspace.workspace.locale = locale.value
}

function playbackModeLabel(mode: string | undefined): string {
  if (mode === 'live') return t('playback.modeLive')
  if (mode === 'replay_paused') return t('playback.replayPaused')
  if (mode === 'replay_playing') return t('playback.replayPlaying')
  return t('playback.modeOffline')
}
</script>

<template>
  <header class="app-toolbar">
    <div class="brand-lockup">
      <span
        class="connection-dot"
        :class="server.connected ? 'bg-emerald-500' : 'bg-red-500'"
        :title="server.connected ? t('connection.connected') : t('connection.disconnected')"
      />
      <strong class="text-sm">{{ t('app.title') }}</strong>
    </div>
    <button class="toolbar-button" :disabled="busy" @click="run(server.setLive)">
      <Radio class="h-4 w-4" />{{ t('playback.live') }}
    </button>
    <button class="toolbar-button" :disabled="busy" @click="run(server.pause)">
      <CirclePause class="h-4 w-4" />{{ t('playback.pause') }}
    </button>
    <button class="toolbar-button" :disabled="busy" @click="run(() => server.play(speed))">
      <CirclePlay class="h-4 w-4" />{{ t('playback.play') }}
    </button>
    <select v-model.number="speed" class="toolbar-select w-20">
      <option v-for="value in [0.1, 0.25, 0.5, 1, 2, 4, 8, 16]" :key="value" :value="value">
        {{ value }}×
      </option>
    </select>
    <span class="mono ml-2 text-xs text-(--text-muted)">
      {{
        playback?.cursor_secs === null || playback?.cursor_secs === undefined
          ? '—'
          : formatRelativeTime(
              playback.cursor_secs - (playback.bounds?.[0] ?? playback.cursor_secs),
            )
      }}
      · {{ playbackModeLabel(playback?.mode) }}
    </span>
    <span
      v-if="playback?.cursor_secs !== null && playback?.cursor_secs !== undefined"
      class="hidden text-xs text-(--text-muted) xl:inline"
    >
      {{ formatAbsoluteTime(playback.cursor_secs, workspace.workspace.locale) }}
    </span>
    <div class="ml-auto flex items-center gap-2">
      <button
        class="icon-button"
        :title="workspace.workspace.left_panel_open ? t('sidebar.hideLeft') : t('sidebar.showLeft')"
        @click="workspace.workspace.left_panel_open = !workspace.workspace.left_panel_open"
      >
        <PanelLeftClose v-if="workspace.workspace.left_panel_open" class="h-4 w-4" />
        <PanelLeftOpen v-else class="h-4 w-4" />
      </button>
      <button
        class="icon-button"
        :title="
          workspace.workspace.right_panel_open ? t('sidebar.hideRight') : t('sidebar.showRight')
        "
        @click="workspace.workspace.right_panel_open = !workspace.workspace.right_panel_open"
      >
        <PanelRightClose v-if="workspace.workspace.right_panel_open" class="h-4 w-4" />
        <PanelRightOpen v-else class="h-4 w-4" />
      </button>
      <button class="icon-button" :title="t('common.refresh')" @click="server.refresh">
        <RefreshCw class="h-4 w-4" />
      </button>
      <button class="toolbar-button" @click="toggleLocale">
        {{ locale === 'zh-CN' ? 'EN' : '中' }}
      </button>
      <button class="toolbar-button" @click="toggleTheme">
        {{ workspace.workspace.theme === 'dark' ? '☀' : '☾' }}
      </button>
    </div>
  </header>
</template>
