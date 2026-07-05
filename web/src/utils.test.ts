import { describe, expect, it } from 'vitest'

import {
  defaultWorkspace,
  effectiveTimeRange,
  extractValue,
  formatAbsoluteTime,
  formatNumber,
  formatRelativeTime,
  niceTickStep,
  normalizeWorkspace,
  selectorKey,
  toAbsoluteTime,
  toRelativeTime,
} from '@/utils'

describe('series helpers', () => {
  it('keeps custom identifiers unambiguous', () => {
    expect(selectorKey({ kind: 'custom', field_id: 'foo.bar' })).toBe('custom:foo.bar')
  })

  it('extracts standard, engine, and custom values', () => {
    const state = {
      position: { x: 1, y: 2, z: 3 },
      engines: [{ index: 2, throttle_lever_ratio: 0.7 }],
      custom_fields: { enabled: { kind: 'bool', value: true } },
      velocity: null,
      attitude: null,
      angular_velocity: null,
      derived: null,
      control_surfaces: null,
    }
    expect(extractValue(state, { kind: 'standard', path: 'position.y' })).toBe(2)
    expect(extractValue(state, { kind: 'engine_throttle', index: 2 })).toBe(0.7)
    expect(extractValue(state, { kind: 'custom', field_id: 'enabled' })).toBe(1)
  })

  it('creates a valid default workspace and formats values', () => {
    expect(defaultWorkspace().max_points).toBe(2000)
    expect(formatNumber(12.3456, 'fixed', 2)).toBe('12.35')
  })

  it('converts and formats absolute and relative timestamps', () => {
    expect(toRelativeTime(1_800_000_012.5, 1_800_000_000)).toBe(12.5)
    expect(toAbsoluteTime(12.5, 1_800_000_000)).toBe(1_800_000_012.5)
    expect(formatRelativeTime(62.25)).toBe('+01:02.250')
    expect(formatAbsoluteTime(12.5)).toBe('t=00:12.500')
    expect(formatAbsoluteTime(1_800_000_000)).not.toContain('1800000000')
    expect(niceTickStep(92, 8)).toBe(20)
  })

  it('drops stale query ranges and normalizes null chart zoom values', () => {
    expect(effectiveTimeRange(0, 1, [1_783_245_238, 1_783_245_267])).toBeNull()
    expect(effectiveTimeRange(5, 15, [10, 20])).toEqual({ start: 10, end: 15 })

    const workspace = defaultWorkspace()
    workspace.charts.push({
      id: 'chart',
      title: 'Chart',
      x: 0,
      y: 0,
      w: 6,
      h: 5,
      legend_visible: true,
      curves: [],
      view: {
        zoom_start: null,
        zoom_end_value: null,
      } as never,
    })
    expect(normalizeWorkspace(workspace).charts[0]?.view).toEqual({})
  })
})
