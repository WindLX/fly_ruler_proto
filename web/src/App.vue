<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import ChartWorkspace from '@/components/ChartWorkspace.vue'
import DataSidebar from '@/components/DataSidebar.vue'
import InspectorPanel from '@/components/InspectorPanel.vue'
import PlaybackToolbar from '@/components/PlaybackToolbar.vue'
import TimelineBar from '@/components/TimelineBar.vue'
import { useSeriesStore } from '@/stores/series'
import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import { extractValue } from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const series = useSeriesStore()
const { locale } = useI18n()

const allCurves = computed(() => [
  ...workspace.workspace.charts.flatMap((chart) => chart.curves),
  ...workspace.workspace.basket,
])

onMounted(async () => {
  await Promise.all([server.refresh(), workspace.load()])
  if (!workspace.workspace.selected_aircraft_id && server.aircraft[0]) {
    workspace.selectAircraft(server.aircraft[0].id)
  }
  locale.value = workspace.workspace.locale
  document.documentElement.dataset.theme = workspace.workspace.theme
  server.connect()
})

onBeforeUnmount(server.stop)

watch(
  () => workspace.workspace.theme,
  (theme) => {
    document.documentElement.dataset.theme = theme
  },
)

watch(
  () => workspace.workspace.locale,
  (value) => {
    locale.value = value
  },
)

watch(
  () => server.workspaceRevision,
  (revision) => {
    workspace.handleRemoteRevision(revision)
  },
)

watch(
  () => server.samples,
  (samples) => {
    if (server.playback?.mode !== 'live') return
    for (const curve of allCurves.value) {
      const sample = samples[curve.aircraft_id]
      if (!sample) continue
      const value = extractValue(sample.state, curve.selector)
      if (value !== null) {
        series.appendLive(curve, sample.timestamp_secs, value, workspace.workspace.max_points)
      }
    }
  },
  { deep: true },
)
</script>

<template>
  <div class="flex h-screen min-h-0 flex-col bg-(--app-bg) text-(--text-primary)">
    <PlaybackToolbar />
    <div
      v-if="server.error"
      class="border-b border-red-500/40 bg-red-500/10 px-3 py-2 text-xs text-red-400"
    >
      {{ server.error }}
    </div>
    <div class="flex min-h-0 flex-1 gap-2 p-2">
      <DataSidebar v-if="workspace.workspace.left_panel_open" />
      <ChartWorkspace />
      <InspectorPanel v-if="workspace.workspace.right_panel_open" />
    </div>
    <TimelineBar />
  </div>
</template>
