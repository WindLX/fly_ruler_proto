import { defineStore } from 'pinia'
import { ref } from 'vue'

import { api } from '@/api'
import type { CurveStyle, SeriesCatalogItem, SeriesData } from '@/types'
import { curveKey } from '@/utils'

export const useSeriesStore = defineStore('series', () => {
  const catalogs = ref<Record<string, SeriesCatalogItem[]>>({})
  const data = ref<Record<string, SeriesData>>({})
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function loadCatalog(aircraftId: string) {
    const response = await api.seriesCatalog(aircraftId)
    catalogs.value = { ...catalogs.value, [aircraftId]: response.fields }
  }

  async function loadCurves(
    curves: CurveStyle[],
    range: { start: number; end: number } | null,
    maxPoints: number,
  ) {
    const unique = [...new Map(curves.map((curve) => [curveKey(curve), curve])).values()]
    if (unique.length === 0) return
    loading.value = true
    error.value = null
    try {
      const response = await api.querySeries(
        unique.map(({ aircraft_id, selector }) => ({ aircraft_id, selector })),
        range,
        maxPoints,
      )
      data.value = {
        ...data.value,
        ...Object.fromEntries(response.series.map((series) => [series.key, series])),
      }
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
    } finally {
      loading.value = false
    }
  }

  function appendLive(curve: CurveStyle, timestamp: number, value: number, maxPoints: number) {
    const key = curveKey(curve)
    const existing = data.value[key]
    const points = [...(existing?.points ?? []), [timestamp, value] as [number, number]]
    if (points.length > maxPoints) points.splice(0, points.length - maxPoints)
    data.value = {
      ...data.value,
      [key]: {
        key,
        aircraft_id: curve.aircraft_id,
        selector: curve.selector,
        points,
        total_points: (existing?.total_points ?? 0) + 1,
        returned_points: points.length,
      },
    }
  }

  return { catalogs, data, loading, error, loadCatalog, loadCurves, appendLive }
})
