import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { mergeSeriesData, useSeriesStore } from '@/stores/series'
import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import type { CurveStyle, SeriesData, TimestampedState } from '@/types'
import { curveKey } from '@/utils'

const curve: CurveStyle = {
  aircraft_id: 'aircraft-1',
  selector: { kind: 'standard', path: 'position.x' },
  alias: 'X',
  color: '#fff',
  line_pattern: 'solid',
  line_width: 2,
  opacity: 1,
  visible: true,
  y_axis: 'left',
  smooth: false,
  show_symbol: false,
  scale: 1,
  offset: 0,
  unit: 'm',
  value_format: 'auto',
  precision: 3,
}

describe('dashboard stores', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('treats websocket-only aircraft samples as available before REST refresh', () => {
    const store = useServerStore()
    store.aircraft = []
    store.samples = {
      'live-aircraft': {
        timestamp_secs: 10,
        state: {
          position: null,
          velocity: null,
          attitude: null,
          angular_velocity: null,
          derived: null,
          control_surfaces: null,
          engines: [],
          custom_fields: {},
        },
      },
    }

    expect(store.availableAircraftIds).toEqual(['live-aircraft'])
  })

  it('deduplicates and orders live points', () => {
    const store = useSeriesStore()
    store.appendLive(curve, 2, 20, 100)
    store.appendLive(curve, 2, 21, 100)
    store.appendLive(curve, 1, 10, 100)
    expect(Object.values(store.data)[0]?.points).toEqual([
      [1, 10],
      [2, 21],
    ])
  })

  it('merges websocket snapshots into configured live curves', () => {
    const store = useSeriesStore()
    const sample: TimestampedState = {
      timestamp_secs: 10,
      state: {
        position: { x: 42, y: 0, z: 0 },
        velocity: null,
        attitude: null,
        angular_velocity: null,
        derived: null,
        control_surfaces: null,
        engines: [],
        custom_fields: {},
      },
    }

    store.mergeLiveSamples([curve], { 'aircraft-1': sample }, 100, true)
    expect(store.data[curveKey(curve)]?.points).toEqual([[10, 42]])

    store.mergeLiveSamples(
      [curve],
      {
        'aircraft-1': {
          ...sample,
          timestamp_secs: 10,
          state: { ...sample.state, position: { x: 43, y: 0, z: 0 } },
        },
      },
      100,
      true,
    )
    expect(store.data[curveKey(curve)]?.points).toEqual([[10, 43]])

    store.mergeLiveSamples(
      [curve],
      {
        'aircraft-1': {
          ...sample,
          timestamp_secs: 11,
          state: { ...sample.state, position: { x: 44, y: 0, z: 0 } },
        },
      },
      100,
      false,
    )
    expect(store.data[curveKey(curve)]?.points).toEqual([[10, 43]])
  })

  it('keeps newer live points when a historical series response arrives late', () => {
    const key = curveKey(curve)
    const existing: SeriesData = {
      key,
      aircraft_id: curve.aircraft_id,
      selector: curve.selector,
      points: [
        [2, 20],
        [4, 40],
      ],
      total_points: 2,
      returned_points: 2,
      stats: { min: 20, max: 40, last: 40, start: 2, end: 4 },
    }
    const incoming: SeriesData = {
      key,
      aircraft_id: curve.aircraft_id,
      selector: curve.selector,
      points: [
        [1, 10],
        [2, 999],
        [3, 30],
      ],
      total_points: 3,
      returned_points: 3,
      stats: { min: 10, max: 999, last: 30, start: 1, end: 3 },
    }

    expect(mergeSeriesData(existing, incoming, 100, 'existing').points).toEqual([
      [1, 10],
      [2, 20],
      [3, 30],
      [4, 40],
    ])
  })

  it('preserves already-loaded history when live points exceed the query point budget', () => {
    const key = curveKey(curve)
    const merged = mergeSeriesData(
      {
        key,
        aircraft_id: curve.aircraft_id,
        selector: curve.selector,
        points: [
          [1, 10],
          [3, 30],
        ],
        total_points: 2,
        returned_points: 2,
        stats: { min: 10, max: 30, last: 30, start: 1, end: 3 },
      },
      {
        key,
        aircraft_id: curve.aircraft_id,
        selector: curve.selector,
        points: [
          [2, 20],
          [4, 40],
        ],
        total_points: 2,
        returned_points: 2,
        stats: { min: 20, max: 40, last: 40, start: 2, end: 4 },
      },
      3,
      'incoming',
    )

    expect(merged.points).toEqual([
      [1, 10],
      [2, 20],
      [3, 30],
      [4, 40],
    ])
    expect(merged.returned_points).toBe(4)
    expect(merged.stats).toMatchObject({ min: 10, max: 40, last: 40, start: 1, end: 4 })
  })

  it('commits grid layout into the persisted chart model', () => {
    const store = useWorkspaceStore()
    store.workspace.charts.push({
      id: 'chart-1',
      title: 'Chart',
      x: 0,
      y: 0,
      w: 4,
      h: 4,
      legend_visible: true,
      curves: [],
      view: {},
    })
    store.updateLayout([{ i: 'chart-1', x: 3, y: 2, w: 7, h: 6 }])
    expect(store.workspace.charts[0]).toMatchObject({ x: 3, y: 2, w: 7, h: 6 })
  })

  it('synchronizes chart zoom when requested', () => {
    const store = useWorkspaceStore()
    store.workspace.charts = ['a', 'b'].map((id) => ({
      id,
      title: id,
      x: 0,
      y: 0,
      w: 4,
      h: 4,
      legend_visible: true,
      curves: [],
      view: {},
    }))
    store.updateChartView('a', { zoom_start_value: 10, zoom_end_value: 20 }, true)
    expect(store.workspace.charts[1]?.view).toEqual({
      zoom_start_value: 10,
      zoom_end_value: 20,
    })
  })

  it('reconciles stale time ranges and single-aircraft curve bindings', () => {
    const store = useWorkspaceStore()
    store.workspace.selected_aircraft_id = 'old-aircraft'
    store.workspace.query_start = 0
    store.workspace.query_end = 1
    store.workspace.charts = [
      {
        id: 'chart',
        title: 'Chart',
        x: 0,
        y: 0,
        w: 4,
        h: 4,
        legend_visible: true,
        curves: [{ ...curve, aircraft_id: 'old-aircraft' }],
        view: {},
      },
    ]

    store.reconcileDataContext(['new-aircraft'], [100, 200])

    expect(store.workspace.selected_aircraft_id).toBe('new-aircraft')
    expect(store.workspace.charts[0]?.curves[0]?.aircraft_id).toBe('new-aircraft')
    expect(store.workspace.query_start).toBeNull()
    expect(store.workspace.query_end).toBeNull()
  })
})
