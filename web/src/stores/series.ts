import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { api } from '@/api'
import type { CurveStyle, SeriesCatalogItem, SeriesData, TimestampedState } from '@/types'
import { curveKey, extractValue } from '@/utils'

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
    if (loadingSignatures.value[signature]) return
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
      const nextData = { ...data.value }
      for (const series of response.series) {
        nextData[series.key] = mergeSeriesData(nextData[series.key], series, maxPoints, 'existing')
      }
      data.value = nextData
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
    data.value = {
      ...data.value,
      [key]: mergeSeriesData(
        data.value[key],
        {
          key,
          aircraft_id: curve.aircraft_id,
          selector: curve.selector,
          points: [[timestamp, value]],
          total_points: 1,
          returned_points: 1,
        },
        maxPoints,
        'incoming',
      ),
    }
  }

  function mergeLiveSamples(
    curves: CurveStyle[],
    samples: Record<string, TimestampedState>,
    maxPoints: number,
    enabled: boolean,
  ) {
    if (!enabled) return
    const unique = [...new Map(curves.map((curve) => [curveKey(curve), curve])).values()]
    const nextData = { ...data.value }
    let changed = false
    for (const curve of unique) {
      const sample = samples[curve.aircraft_id]
      if (!sample) continue
      const value = extractValue(sample.state, curve.selector)
      if (value === null || !Number.isFinite(sample.timestamp_secs)) continue
      const key = curveKey(curve)
      nextData[key] = mergeSeriesData(
        nextData[key],
        {
          key,
          aircraft_id: curve.aircraft_id,
          selector: curve.selector,
          points: [[sample.timestamp_secs, value]],
          total_points: 1,
          returned_points: 1,
        },
        maxPoints,
        'incoming',
      )
      changed = true
    }
    if (changed) data.value = nextData
  }

  function clear() {
    data.value = {}
    loadingSignatures.value = {}
    errors.value = {}
    error.value = null
    requestGenerations.clear()
  }

  return {
    catalogs,
    data,
    loading,
    error,
    loadCatalog,
    loadCurves,
    appendLive,
    mergeLiveSamples,
    clear,
    isLoading,
    errorFor,
  }
})

function curveSignature(curves: CurveStyle[]): string {
  return [...new Set(curves.map(curveKey))].sort().join('|')
}

export function mergeSeriesData(
  existing: SeriesData | undefined,
  incoming: SeriesData,
  maxPoints: number,
  duplicatePreference: 'existing' | 'incoming',
): SeriesData {
  const points = mergePoints(
    existing?.points ?? [],
    incoming.points,
    maxPoints,
    duplicatePreference,
  )
  return {
    key: incoming.key,
    aircraft_id: incoming.aircraft_id,
    selector: incoming.selector,
    points,
    total_points: Math.max(existing?.total_points ?? 0, incoming.total_points, points.length),
    returned_points: points.length,
    stats: computeStats(points),
  }
}

function mergePoints(
  existing: Array<[number, number]>,
  incoming: Array<[number, number]>,
  maxPoints: number,
  duplicatePreference: 'existing' | 'incoming',
): Array<[number, number]> {
  const byTimestamp = new Map<number, [number, number]>()
  const add = ([timestamp, value]: [number, number]) => {
    if (Number.isFinite(timestamp) && Number.isFinite(value))
      byTimestamp.set(timestamp, [timestamp, value])
  }
  if (duplicatePreference === 'incoming') {
    existing.forEach(add)
    incoming.forEach(add)
  } else {
    incoming.forEach(add)
    existing.forEach(add)
  }
  const points = [...byTimestamp.values()].sort((left, right) => left[0] - right[0])
  if (points.length > maxPoints) points.splice(0, points.length - maxPoints)
  return points
}

function computeStats(points: Array<[number, number]>): SeriesData['stats'] {
  if (points.length === 0) return null
  let min = Number.POSITIVE_INFINITY
  let max = Number.NEGATIVE_INFINITY
  for (const [, value] of points) {
    min = Math.min(min, value)
    max = Math.max(max, value)
  }
  const first = points[0]!
  const last = points[points.length - 1]!
  return { min, max, last: last[1], start: first[0], end: last[0] }
}
