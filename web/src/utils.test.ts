import { describe, expect, it } from 'vitest'

import {
  defaultWorkspace,
  effectiveTimeRange,
  eventFrameWindowSecs,
  extractValue,
  formatAbsoluteTime,
  formatNumber,
  formatRelativeTime,
  generateTimelineTicks,
  niceTickStep,
  normalizeWorkspace,
  selectorKey,
  toAbsoluteTime,
  toRelativeTime,
  timelineTickStep,
  zoomTimeRange,
} from '@/utils'

describe('series helpers', () => {
  it('keeps propulsor identifiers unambiguous', () => {
    expect(
      selectorKey({
        kind: 'propulsor',
        propulsor_id: 'engine.left',
        field: 'throttle_ratio',
      }),
    ).toBe('propulsor:engine.left:throttle_ratio')
  })

  it('extracts standard and propulsor values', () => {
    const state = {
      position: { x: 1, y: 2, z: 3 },
      propulsors: [
        {
          propulsor_id: 'engine.left',
          kind: 1,
          throttle_ratio: 0.7,
          rpm: null,
          blade_pitch_rad: null,
          thrust_newton: null,
          torque_newton_meter: null,
          index: 1,
        },
      ],
      velocity: null,
      attitude: null,
      angular_velocity: null,
      derived: null,
      control_surfaces: null,
    }
    expect(extractValue(state, { kind: 'standard', path: 'position.y' })).toBe(2)
    expect(
      extractValue(state, {
        kind: 'propulsor',
        propulsor_id: 'engine.left',
        field: 'throttle_ratio',
      }),
    ).toBe(0.7)
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

  it('generates pixel-aware major and minor timeline ticks', () => {
    expect(timelineTickStep(30, 1_256)).toBe(2)
    expect(timelineTickStep(30, 600)).toBe(5)
    const ticks = generateTimelineTicks(30, 1_256)
    const major = ticks.filter((tick) => tick.level === 'major')
    const minor = ticks.filter((tick) => tick.level === 'minor')
    expect(major.length).toBeGreaterThan(10)
    expect(minor.length).toBeGreaterThan(major.length)
    expect(major.some((tick) => tick.label === '+00:10')).toBe(true)

    const offsetTicks = generateTimelineTicks(10, 800, 84, 10)
    expect(offsetTicks.some((tick) => tick.label === '+00:12')).toBe(true)
    expect(offsetTicks.every((tick) => tick.value >= 10 && tick.value <= 20)).toBe(true)
  })

  it('derives a bounded event frame window from timeline scale', () => {
    expect(eventFrameWindowSecs([0, 30], 1_000)).toBeCloseTo(0.3)
    expect(eventFrameWindowSecs([0, 3_600], 1_000)).toBe(0.5)
    expect(eventFrameWindowSecs(null)).toBe(1e-6)
  })

  it('zooms a time range around the requested anchor and clamps to bounds', () => {
    expect(zoomTimeRange([0, 100], [0, 100], 25, 0.5)).toEqual([12.5, 62.5])
    expect(zoomTimeRange([0, 100], [12.5, 62.5], 12.5, 0.5)).toEqual([12.5, 37.5])
    expect(zoomTimeRange([0, 100], [12.5, 37.5], 20, 100)).toEqual([0, 100])
  })
})
