import { defineStore } from 'pinia'
import { computed, shallowRef, triggerRef } from 'vue'

import { api } from '@/api'
import type { CurveStyle, SeriesCatalogItem, SeriesData } from '@/types'
import { curveKey } from '@/utils'

export const useSeriesStore = defineStore('series', () => {
  const catalogs = shallowRef<Record<string, SeriesCatalogItem[]>>({})
  const data = shallowRef<Record<string, SeriesData>>({})
  const loadingSignatures = shallowRef<Record<string, boolean>>({})
  const errors = shallowRef<Record<string, string | null>>({})
  const error = shallowRef<string | null>(null)
  const requestGenerations = new Map<string, number>()
  const liveCursors = new Map<string, number>()
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
    maxPoints: number | null,
    append = false,
  ) {
    const unique = [...new Map(curves.map((curve) => [curveKey(curve), curve])).values()]
    if (unique.length === 0) return
    const signature = `${curveSignature(unique)}|${range?.start ?? 'all'}:${range?.end ?? 'all'}|${maxPoints}`
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
        const next = append ? mergeSeriesData(data.value[series.key], series, 'existing') : series
        data.value[series.key] = maxPoints === null ? next : downsampleSeriesData(next, maxPoints)
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

  function clear() {
    data.value = {}
    loadingSignatures.value = {}
    errors.value = {}
    error.value = null
    requestGenerations.clear()
    liveCursors.clear()
  }

  async function catchUpLiveCurves(
    curves: CurveStyle[],
    bounds: [number, number] | null,
    maxPoints: number,
  ) {
    if (!bounds || curves.length === 0) return
    const unique = [...new Map(curves.map((curve) => [curveKey(curve), curve])).values()]
    const cursors = unique
      .map((curve) => liveCursors.get(curveKey(curve)))
      .filter((cursor): cursor is number => cursor !== undefined)
    const start = cursors.length === unique.length ? Math.min(...cursors) : bounds[0]
    await loadCurves(unique, { start, end: bounds[1] }, maxPoints, true)
    for (const curve of unique) {
      const points = data.value[curveKey(curve)]?.points ?? []
      const latest = points[points.length - 1]?.[0]
      if (latest !== undefined) liveCursors.set(curveKey(curve), latest)
    }
  }

  return {
    catalogs,
    data,
    loading,
    error,
    loadCatalog,
    loadCurves,
    catchUpLiveCurves,
    clear,
    isLoading,
    errorFor,
  }
})

function curveSignature(curves: CurveStyle[]): string {
  return [...new Set(curves.map(curveKey))].sort().join('|')
}

export function downsampleSeriesData(series: SeriesData, maxPoints: number): SeriesData {
  if (series.points.length <= maxPoints) return series
  const points = lttb(series.points, maxPoints)
  return {
    ...series,
    points,
    returned_points: points.length,
    stats: computeStats(points),
  }
}

function lttb(points: Array<[number, number]>, threshold: number): Array<[number, number]> {
  if (points.length <= threshold || threshold < 3) return points
  const sampled: Array<[number, number]> = [points[0]!]
  const every = (points.length - 2) / (threshold - 2)
  let selected = 0
  for (let bucket = 0; bucket < threshold - 2; bucket++) {
    const averageStart = Math.min(Math.floor((bucket + 1) * every) + 1, points.length)
    const averageEnd = Math.min(Math.floor((bucket + 2) * every) + 1, points.length)
    const averageSlice = points.slice(averageStart, averageEnd)
    const fallback = points[Math.min(Math.max(averageStart - 1, 0), points.length - 1)]!
    const average = averageSlice.reduce(
      (sum, point) => [sum[0] + point[0], sum[1] + point[1]] as [number, number],
      [0, 0] as [number, number],
    )
    const averageX = averageSlice.length === 0 ? fallback[0] : average[0] / averageSlice.length
    const averageY = averageSlice.length === 0 ? fallback[1] : average[1] / averageSlice.length
    const rangeStart = Math.floor(bucket * every) + 1
    const rangeEnd = Math.min(Math.floor((bucket + 1) * every) + 1, points.length - 1)
    const anchor = points[selected]!
    let maxArea = -1
    let next = rangeStart
    for (let index = rangeStart; index < Math.max(rangeEnd, rangeStart + 1); index++) {
      const candidateIndex = Math.min(index, points.length - 2)
      const point = points[candidateIndex]!
      const area = Math.abs(
        (anchor[0] - averageX) * (point[1] - anchor[1]) -
          (anchor[0] - point[0]) * (averageY - anchor[1]),
      )
      if (area > maxArea) {
        maxArea = area
        next = candidateIndex
      }
    }
    sampled.push(points[next]!)
    selected = next
  }
  sampled.push(points[points.length - 1]!)
  return sampled
}

export function mergeSeriesData(
  existing: SeriesData | undefined,
  incoming: SeriesData,
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

function pushDeduped(points: Array<[number, number]>, point: [number, number]): void {
  const previous = points[points.length - 1]
  if (previous?.[0] === point[0]) previous[1] = point[1]
  else points.push([point[0], point[1]])
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
