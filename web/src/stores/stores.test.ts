import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useSeriesStore } from '@/stores/series'
import { useWorkspaceStore } from '@/stores/workspace'
import type { CurveStyle } from '@/types'

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
})
