export interface RuntimeConfig {
  api_base_url: string
  websocket_url: string
}

const defaults: RuntimeConfig = {
  api_base_url: '/api/v1',
  websocket_url: '/api/v1/ws',
}

export function readRuntimeConfig(documentRef: Document = document): RuntimeConfig {
  const source = documentRef.getElementById('fly-ruler-runtime-config')?.textContent?.trim()
  if (!source || source === '__FLY_RULER_RUNTIME_CONFIG__') return defaults
  try {
    const parsed = JSON.parse(source) as Partial<RuntimeConfig>
    return {
      api_base_url: normalizeBase(parsed.api_base_url ?? defaults.api_base_url),
      websocket_url: parsed.websocket_url ?? defaults.websocket_url,
    }
  } catch {
    return defaults
  }
}

function normalizeBase(value: string): string {
  return value.length > 1 ? value.replace(/\/+$/, '') : value
}

export function resolveWebSocketUrl(value: string, locationRef: Location = location): string {
  const url = new URL(value, locationRef.href)
  if (url.protocol === 'http:') url.protocol = 'ws:'
  if (url.protocol === 'https:') url.protocol = 'wss:'
  return url.toString()
}

export const runtimeConfig = typeof document === 'undefined' ? defaults : readRuntimeConfig()
