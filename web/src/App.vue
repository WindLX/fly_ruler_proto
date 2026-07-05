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
import { usePlaybackShortcuts } from '@/usePlaybackShortcuts'

const server = useServerStore()
const workspace = useWorkspaceStore()
const series = useSeriesStore()
const { locale, t, te } = useI18n()
usePlaybackShortcuts()

const allCurves = computed(() => [
  ...workspace.workspace.charts.flatMap((chart) => chart.curves),
  ...workspace.workspace.basket,
])

onMounted(async () => {
  await Promise.all([server.refresh(), workspace.load()])
  workspace.reconcileDataContext(
    server.aircraft.map((aircraft) => aircraft.id),
    server.playback?.bounds ?? null,
  )
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
  [
    () => server.aircraft.map((aircraft) => aircraft.id).join('|'),
    () => server.playback?.bounds?.[0] ?? null,
    () => server.playback?.bounds?.[1] ?? null,
    () => server.storeRevision,
  ],
  ([aircraftIds, start, end]) => {
    const ids = typeof aircraftIds === 'string' ? aircraftIds.split('|').filter(Boolean) : []
    const bounds =
      typeof start === 'number' && typeof end === 'number'
        ? ([start, end] as [number, number])
        : null
    if (workspace.hydrated) workspace.reconcileDataContext(ids, bounds)
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
  <div class="app-shell">
    <PlaybackToolbar />
    <div v-if="server.error" class="app-error">
      {{ te(server.error) ? t(server.error) : server.error }}
    </div>
    <div class="workbench">
      <DataSidebar v-if="workspace.workspace.left_panel_open" />
      <ChartWorkspace />
      <InspectorPanel v-if="workspace.workspace.right_panel_open" />
    </div>
    <TimelineBar />
  </div>
</template>
