import { defineStore } from 'pinia'
import { computed, nextTick, ref, watch } from 'vue'

import { api } from '@/api'
import type { ChartModel, CurveStyle, WorkspaceSnapshot } from '@/types'
import {
  curveKey,
  defaultWorkspace,
  effectiveTimeRange,
  normalizeChartView,
  normalizeWorkspace,
} from '@/utils'

export const useWorkspaceStore = defineStore('workspace', () => {
  const workspace = ref<WorkspaceSnapshot>(defaultWorkspace())
  const revision = ref(0)
  const hydrated = ref(false)
  const saving = ref(false)
  let applyingRemote = false
  let saveTimer: number | null = null
  const localGeneration = ref(0)
  const savedGeneration = ref(0)
  let pendingRemoteRevision = 0
  const dirty = computed(() => localGeneration.value !== savedGeneration.value)
  const selectedChart = computed(
    () =>
      workspace.value.charts.find((chart) => chart.id === workspace.value.selected_chart_id) ??
      null,
  )

  async function load() {
    const response = await api.workspace()
    applyingRemote = true
    if (response.workspace) {
      workspace.value = normalizeWorkspace(response.workspace.workspace)
      revision.value = response.workspace.revision
    }
    hydrated.value = true
    await nextTick()
    localGeneration.value = 0
    savedGeneration.value = 0
    applyingRemote = false
  }

  async function save() {
    if (!hydrated.value || applyingRemote || saving.value || !dirty.value) return
    saving.value = true
    try {
      while (savedGeneration.value !== localGeneration.value) {
        const generation = localGeneration.value
        const snapshot = JSON.parse(JSON.stringify(workspace.value)) as WorkspaceSnapshot
        const response = await api.saveWorkspace(snapshot)
        revision.value = response.workspace.revision
        savedGeneration.value = generation
      }
    } finally {
      saving.value = false
    }
    if (pendingRemoteRevision > revision.value && !dirty.value) {
      pendingRemoteRevision = 0
      await load()
    } else {
      pendingRemoteRevision = 0
    }
  }

  function scheduleSave() {
    if (!hydrated.value || applyingRemote) return
    localGeneration.value++
    if (saveTimer !== null) return
    saveTimer = window.setTimeout(() => {
      saveTimer = null
      void save()
    }, 600)
  }

  function handleRemoteRevision(nextRevision: number) {
    if (nextRevision <= revision.value) return
    pendingRemoteRevision = Math.max(pendingRemoteRevision, nextRevision)
    if (!saving.value && !dirty.value) {
      pendingRemoteRevision = 0
      void load()
    }
  }

  function selectAircraft(id: string | null) {
    workspace.value.selected_aircraft_id = id
  }

  function reconcileDataContext(aircraftIds: string[], bounds: [number, number] | null): void {
    const before = JSON.stringify(workspace.value)
    const available = new Set(aircraftIds)
    if (
      workspace.value.selected_aircraft_id !== null &&
      !available.has(workspace.value.selected_aircraft_id)
    ) {
      workspace.value.selected_aircraft_id = aircraftIds[0] ?? null
    } else if (workspace.value.selected_aircraft_id === null && aircraftIds[0]) {
      workspace.value.selected_aircraft_id = aircraftIds[0]
    }

    if (aircraftIds.length === 1) {
      const onlyAircraft = aircraftIds[0]!
      for (const curve of [
        ...workspace.value.charts.flatMap((chart) => chart.curves),
        ...workspace.value.basket,
      ]) {
        if (!available.has(curve.aircraft_id)) curve.aircraft_id = onlyAircraft
      }
    }

    const range = effectiveTimeRange(workspace.value.query_start, workspace.value.query_end, bounds)
    if (workspace.value.query_start !== null || workspace.value.query_end !== null) {
      workspace.value.query_start = range?.start ?? null
      workspace.value.query_end = range?.end ?? null
    }
    for (const chart of workspace.value.charts) {
      const normalizedView = normalizeChartView(chart.view)
      if (!sameChartView(chart.view, normalizedView)) chart.view = normalizedView
    }
    if (JSON.stringify(workspace.value) !== before) scheduleSave()
  }

  function addToBasket(curve: CurveStyle) {
    if (!workspace.value.basket.some((item) => curveKey(item) === curveKey(curve))) {
      workspace.value.basket.push(curve)
    }
  }

  function createChart(title?: string) {
    const id = `chart-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`
    const chart: ChartModel = {
      id,
      title: title ?? `Chart ${workspace.value.charts.length + 1}`,
      x: 0,
      y: workspace.value.charts.length * 5,
      w: 6,
      h: 5,
      legend_visible: true,
      curves: workspace.value.basket.map((curve) => ({ ...curve })),
      view: {},
    }
    workspace.value.charts.push(chart)
    workspace.value.selected_chart_id = id
    workspace.value.basket = []
  }

  function addBasketToSelected(fallbackTitle?: string) {
    const chart = selectedChart.value
    if (!chart) return createChart(fallbackTitle)
    const existing = new Set(chart.curves.map(curveKey))
    for (const curve of workspace.value.basket) {
      if (!existing.has(curveKey(curve))) chart.curves.push({ ...curve })
    }
    workspace.value.basket = []
  }

  function removeChart(id: string) {
    workspace.value.charts = workspace.value.charts.filter((chart) => chart.id !== id)
    if (workspace.value.selected_chart_id === id) {
      workspace.value.selected_chart_id = workspace.value.charts[0]?.id ?? null
    }
  }

  function updateLayout(layout: Array<{ i: string; x: number; y: number; w: number; h: number }>) {
    const byId = new Map(layout.map((item) => [item.i, item]))
    for (const chart of workspace.value.charts) {
      const item = byId.get(chart.id)
      if (
        item &&
        (chart.x !== item.x || chart.y !== item.y || chart.w !== item.w || chart.h !== item.h)
      ) {
        Object.assign(chart, { x: item.x, y: item.y, w: item.w, h: item.h })
      }
    }
  }

  function updateChartView(
    chartId: string,
    view: ChartModel['view'],
    synchronize = workspace.value.sync_charts,
  ) {
    const chart = workspace.value.charts.find((item) => item.id === chartId)
    if (!chart) return
    const normalized = normalizeChartView(view)
    if (!sameChartView(chart.view, normalized)) chart.view = normalized
    if (synchronize) {
      for (const other of workspace.value.charts) {
        if (other.id !== chartId && !sameChartView(other.view, normalized)) {
          other.view = { ...normalized }
        }
      }
    }
  }

  watch(workspace, scheduleSave, { deep: true })

  return {
    workspace,
    revision,
    hydrated,
    saving,
    dirty,
    selectedChart,
    load,
    save,
    handleRemoteRevision,
    selectAircraft,
    reconcileDataContext,
    addToBasket,
    createChart,
    addBasketToSelected,
    removeChart,
    updateLayout,
    updateChartView,
  }
})

function sameChartView(left: ChartModel['view'], right: ChartModel['view']): boolean {
  return (
    sameOptionalNumber(left.zoom_start, right.zoom_start) &&
    sameOptionalNumber(left.zoom_end, right.zoom_end) &&
    sameOptionalNumber(left.zoom_start_value, right.zoom_start_value) &&
    sameOptionalNumber(left.zoom_end_value, right.zoom_end_value)
  )
}

function sameOptionalNumber(left: number | undefined, right: number | undefined): boolean {
  if (left === undefined || right === undefined) return left === right
  return Math.abs(left - right) <= 1e-6
}
