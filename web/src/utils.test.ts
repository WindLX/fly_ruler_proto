import { describe, expect, it } from 'vitest'

import { defaultWorkspace, extractValue, formatNumber, selectorKey } from '@/utils'

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
})
