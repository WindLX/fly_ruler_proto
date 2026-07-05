import { describe, expect, it } from 'vitest'

import { messages } from '@/i18n'

function leafKeys(value: unknown, prefix = ''): string[] {
  if (!value || typeof value !== 'object') return [prefix]
  return Object.entries(value).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key),
  )
}

describe('i18n resources', () => {
  it('keeps English and Chinese resource structures in sync', () => {
    expect(leafKeys(messages.en).sort()).toEqual(leafKeys(messages['zh-CN']).sort())
  })
})
