import { describe, expect, it } from 'vitest'

import { readRuntimeConfig, resolveWebSocketUrl } from '@/runtime'

describe('runtime configuration', () => {
  it('reads the Rust-injected JSON document', () => {
    const documentRef = {
      getElementById: () => ({
        textContent: JSON.stringify({
          api_base_url: 'https://example.test/api/v1/',
          websocket_url: 'wss://example.test/api/v1/ws',
        }),
      }),
    } as unknown as Document
    expect(readRuntimeConfig(documentRef)).toEqual({
      api_base_url: 'https://example.test/api/v1',
      websocket_url: 'wss://example.test/api/v1/ws',
    })
  })

  it('falls back safely and resolves same-origin websocket paths', () => {
    const documentRef = {
      getElementById: () => ({ textContent: '__FLY_RULER_RUNTIME_CONFIG__' }),
    } as unknown as Document
    expect(readRuntimeConfig(documentRef).api_base_url).toBe('/api/v1')
    const locationRef = { href: 'https://console.example.test/dashboard' } as Location
    expect(resolveWebSocketUrl('/api/v1/ws', locationRef)).toBe(
      'wss://console.example.test/api/v1/ws',
    )
  })
})
