import { defineStore } from 'pinia'
import { computed, nextTick, ref, watch } from 'vue'

import { api } from '@/api'
import type { ChartModel, CurveStyle, WorkspaceSnapshot } from '@/types'
import { curveKey, defaultWorkspace } from '@/utils'

let saveTimer: number | null = null

export const useWorkspaceStore = defineStore('workspace', () => {
  const workspace = ref<WorkspaceSnapshot>(defaultWorkspace())
  const revision = ref(0)
  const hydrated = ref(false)
  const saving = ref(false)
  let applyingRemote = false
  const selectedChart = computed(
    () =>
      workspace.value.charts.find((chart) => chart.id === workspace.value.selected_chart_id) ??
      null,
  )

  async function load() {
    const response = await api.workspace()
    applyingRemote = true
    if (response.workspace) {
      workspace.value = response.workspace.workspace
      revision.value = response.workspace.revision
    }
    hydrated.value = true
    await nextTick()
    applyingRemote = false
  }

  async function save() {
    if (!hydrated.value || applyingRemote) return
    saving.value = true
    try {
      const response = await api.saveWorkspace(workspace.value)
      revision.value = response.workspace.revision
    } finally {
      saving.value = false
    }
  }

  function scheduleSave() {
    if (!hydrated.value || applyingRemote) return
    if (saveTimer !== null) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => void save(), 600)
  }

  function selectAircraft(id: string | null) {
    workspace.value.selected_aircraft_id = id
  }

  function addToBasket(curve: CurveStyle) {
    if (!workspace.value.basket.some((item) => curveKey(item) === curveKey(curve))) {
      workspace.value.basket.push(curve)
    }
  }

  function createChart() {
    const id = `chart-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`
    const chart: ChartModel = {
      id,
      title: `Chart ${workspace.value.charts.length + 1}`,
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

  function addBasketToSelected() {
    const chart = selectedChart.value
    if (!chart) return createChart()
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
      if (item) Object.assign(chart, { x: item.x, y: item.y, w: item.w, h: item.h })
    }
  }

  watch(workspace, scheduleSave, { deep: true })

  return {
    workspace,
    revision,
    hydrated,
    saving,
    selectedChart,
    load,
    save,
    selectAircraft,
    addToBasket,
    createChart,
    addBasketToSelected,
    removeChart,
    updateLayout,
  }
})
