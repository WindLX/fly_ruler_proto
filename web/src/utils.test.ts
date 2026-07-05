import { describe, expect, it } from 'vitest'

import {
  defaultWorkspace,
  extractValue,
  formatAbsoluteTime,
  formatNumber,
  formatRelativeTime,
  niceTickStep,
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
})
