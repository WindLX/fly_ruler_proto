import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { api } from '@/api'
import type { CurveStyle, SeriesCatalogItem, SeriesData } from '@/types'
import { curveKey } from '@/utils'

export const useSeriesStore = defineStore('series', () => {
  const catalogs = ref<Record<string, SeriesCatalogItem[]>>({})
  const data = ref<Record<string, SeriesData>>({})
  const loadingSignatures = ref<Record<string, boolean>>({})
  const errors = ref<Record<string, string | null>>({})
  const error = ref<string | null>(null)
  const requestGenerations = new Map<string, number>()
  const loading = computed(() => Object.values(loadingSignatures.value).some(Boolean))

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
    const signature = curveSignature(unique)
    const generation = (requestGenerations.get(signature) ?? 0) + 1
    requestGenerations.set(signature, generation)
    loadingSignatures.value = { ...loadingSignatures.value, [signature]: true }
    errors.value = { ...errors.value, [signature]: null }
    error.value = null
    try {
      const response = await api.querySeries(
        unique.map(({ aircraft_id, selector }) => ({ aircraft_id, selector })),
        range,
        maxPoints,
      )
      if (requestGenerations.get(signature) !== generation) return
      data.value = {
        ...data.value,
        ...Object.fromEntries(response.series.map((series) => [series.key, series])),
      }
    } catch (cause) {
      if (requestGenerations.get(signature) !== generation) return
      const message = cause instanceof Error ? cause.message : String(cause)
      error.value = message
      errors.value = { ...errors.value, [signature]: message }
    } finally {
      if (requestGenerations.get(signature) === generation) {
        loadingSignatures.value = { ...loadingSignatures.value, [signature]: false }
      }
    }
  }

  function isLoading(curves: CurveStyle[]): boolean {
    return loadingSignatures.value[curveSignature(curves)] ?? false
  }

  function errorFor(curves: CurveStyle[]): string | null {
    return errors.value[curveSignature(curves)] ?? null
  }

  function appendLive(curve: CurveStyle, timestamp: number, value: number, maxPoints: number) {
    if (!Number.isFinite(timestamp) || !Number.isFinite(value)) return
    const key = curveKey(curve)
    const existing = data.value[key]
    const points = [...(existing?.points ?? [])]
    const next: [number, number] = [timestamp, value]
    const last = points[points.length - 1]
    if (!last || timestamp > last[0]) {
      points.push(next)
    } else if (timestamp === last[0]) {
      points[points.length - 1] = next
    } else {
      const index = points.findIndex(([time]) => time >= timestamp)
      if (index >= 0 && points[index]?.[0] === timestamp) points[index] = next
      else points.splice(index < 0 ? points.length : index, 0, next)
    }
    if (points.length > maxPoints) points.splice(0, points.length - maxPoints)
    data.value = {
      ...data.value,
      [key]: {
        key,
        aircraft_id: curve.aircraft_id,
        selector: curve.selector,
        points,
        total_points: Math.max(existing?.total_points ?? 0, points.length),
        returned_points: points.length,
      },
    }
  }

  return {
    catalogs,
    data,
    loading,
    error,
    loadCatalog,
    loadCurves,
    appendLive,
    isLoading,
    errorFor,
  }
})

function curveSignature(curves: CurveStyle[]): string {
  return [...new Set(curves.map(curveKey))].sort().join('|')
}
