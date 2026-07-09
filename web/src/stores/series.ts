import { defineStore } from 'pinia'
import { computed, shallowRef, triggerRef } from 'vue'

import { api } from '@/api'
import type { CurveStyle, SeriesCatalogItem, SeriesData, TimestampedState } from '@/types'
import { curveKey, extractValue } from '@/utils'

export const useSeriesStore = defineStore('series', () => {
  const catalogs = shallowRef<Record<string, SeriesCatalogItem[]>>({})
  const data = shallowRef<Record<string, SeriesData>>({})
  const loadingSignatures = shallowRef<Record<string, boolean>>({})
  const errors = shallowRef<Record<string, string | null>>({})
  const error = shallowRef<string | null>(null)
  const requestGenerations = new Map<string, number>()
  const loading = computed(() => Object.values(loadingSignatures.value).some(Boolean))

  async function loadCatalog(aircraftId: string) {
    if (catalogs.value[aircraftId]) return
    try {
      const response = await api.seriesCatalog(aircraftId)
      catalogs.value = { ...catalogs.value, [aircraftId]: response.fields }
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause)
      if (message.includes('aircraft not found')) {
        const next = { ...catalogs.value }
        delete next[aircraftId]
        catalogs.value = next
        return
      }
      throw cause
    }
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
      for (const series of response.series) {
        data.value[series.key] = mergeSeriesData(
          data.value[series.key],
          series,
          maxPoints,
          'existing',
        )
      }
      triggerRef(data)
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
    mergeLivePoint(data.value, curve, key, timestamp, value, maxPoints)
    triggerRef(data)
  }

  function mergeLiveSamples(
    curves: CurveStyle[],
    samples: Record<string, TimestampedState>,
    maxPoints: number,
    enabled: boolean,
  ) {
    if (!enabled) return
    const unique = [...new Map(curves.map((curve) => [curveKey(curve), curve])).values()]
    let changed = false
    for (const curve of unique) {
      const sample = samples[curve.aircraft_id]
      if (!sample) continue
      const value = extractValue(sample.state, curve.selector)
      if (value === null || !Number.isFinite(sample.timestamp_secs)) continue
      const key = curveKey(curve)
      changed =
        mergeLivePoint(data.value, curve, key, sample.timestamp_secs, value, maxPoints) || changed
    }
    if (changed) triggerRef(data)
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
  _maxPoints: number,
  duplicatePreference: 'existing' | 'incoming',
): SeriesData {
  const points = mergePoints(existing?.points ?? [], incoming.points, duplicatePreference)
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
  duplicatePreference: 'existing' | 'incoming',
): Array<[number, number]> {
  const left = existing.filter(
    ([timestamp, value]) => Number.isFinite(timestamp) && Number.isFinite(value),
  )
  const right = incoming.filter(
    ([timestamp, value]) => Number.isFinite(timestamp) && Number.isFinite(value),
  )
  left.sort((a, b) => a[0] - b[0])
  right.sort((a, b) => a[0] - b[0])
  const points: Array<[number, number]> = []
  let leftIndex = 0
  let rightIndex = 0
  while (leftIndex < left.length || rightIndex < right.length) {
    const existingPoint = left[leftIndex]
    const incomingPoint = right[rightIndex]
    if (!incomingPoint || (existingPoint && existingPoint[0] < incomingPoint[0])) {
      pushDeduped(points, existingPoint!)
      leftIndex++
    } else if (!existingPoint || incomingPoint[0] < existingPoint[0]) {
      pushDeduped(points, incomingPoint)
      rightIndex++
    } else {
      pushDeduped(points, duplicatePreference === 'existing' ? existingPoint : incomingPoint)
      leftIndex++
      rightIndex++
    }
  }
  return points
}

function mergeLivePoint(
  data: Record<string, SeriesData>,
  curve: CurveStyle,
  key: string,
  timestamp: number,
  value: number,
  _maxPoints: number,
): boolean {
  const existing = data[key]
  if (!existing) {
    data[key] = {
      key,
      aircraft_id: curve.aircraft_id,
      selector: curve.selector,
      points: [[timestamp, value]],
      total_points: 1,
      returned_points: 1,
      stats: { min: value, max: value, last: value, start: timestamp, end: timestamp },
    }
    return true
  }
  const points = existing.points
  const last = points[points.length - 1]
  if (!last || timestamp > last[0]) {
    points.push([timestamp, value])
    existing.returned_points = points.length
    existing.total_points = Math.max(existing.total_points, points.length)
    existing.stats = updateStatsForAppend(existing.stats ?? null, timestamp, value)
    return true
  }
  if (timestamp === last[0]) {
    if (last[1] === value) return false
    last[1] = value
    existing.stats = computeStats(points)
    existing.returned_points = points.length
    existing.total_points = Math.max(existing.total_points, points.length)
    return true
  }
  const index = lowerBound(points, timestamp)
  if (points[index]?.[0] === timestamp) {
    if (points[index]![1] === value) return false
    points[index]![1] = value
  } else {
    points.splice(index, 0, [timestamp, value])
  }
  existing.returned_points = points.length
  existing.total_points = Math.max(existing.total_points, points.length)
  existing.stats = computeStats(points)
  return true
}

function lowerBound(points: Array<[number, number]>, timestamp: number): number {
  let low = 0
  let high = points.length
  while (low < high) {
    const middle = (low + high) >> 1
    if (points[middle]![0] < timestamp) low = middle + 1
    else high = middle
  }
  return low
}

function pushDeduped(points: Array<[number, number]>, point: [number, number]): void {
  const previous = points[points.length - 1]
  if (previous?.[0] === point[0]) previous[1] = point[1]
  else points.push([point[0], point[1]])
}

function updateStatsForAppend(
  stats: SeriesData['stats'],
  timestamp: number,
  value: number,
): NonNullable<SeriesData['stats']> {
  if (!stats) return { min: value, max: value, last: value, start: timestamp, end: timestamp }
  return {
    min: Math.min(stats.min, value),
    max: Math.max(stats.max, value),
    last: value,
    start: Math.min(stats.start, timestamp),
    end: Math.max(stats.end, timestamp),
  }
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
