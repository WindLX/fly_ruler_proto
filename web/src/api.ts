import type {
  AircraftSummary,
  AircraftEvent,
  OperationRecord,
  SeriesCatalogItem,
  SeriesData,
  SeriesSelection,
  ServerStatus,
  SessionSummary,
  WorkspaceDocument,
  WorkspaceSnapshot,
  PlaybackStepDirection,
  PlaybackStepUnit,
} from '@/types'
import { resolveWebSocketUrl, runtimeConfig } from '@/runtime'

interface ApiErrorBody {
  code?: string
  message?: string
  details?: unknown
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${runtimeConfig.api_base_url}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  })
  if (!response.ok) {
    const error = (await response.json().catch(() => null)) as ApiErrorBody | null
    throw new Error(error?.message ?? `${response.status} ${response.statusText}`)
  }
  return (await response.json()) as T
}

export const api = {
  status: () => apiFetch<ServerStatus>('/status'),
  aircraft: () => apiFetch<{ aircraft: AircraftSummary[] }>('/aircraft'),
  sessions: () => apiFetch<{ sessions: SessionSummary[] }>('/sessions'),
  aircraftEvents: (aircraftId: string) =>
    apiFetch<{ items: AircraftEvent[]; total: number }>(
      `/aircraft/${encodeURIComponent(aircraftId)}/events?offset=0&limit=100`,
    ),
  timelineEvents: (start: number, end: number, offset = 0, limit = 10_000) =>
    apiFetch<{ items: AircraftEvent[]; total: number; offset: number; limit: number }>(
      `/timeline/events?start=${encodeURIComponent(start)}&end=${encodeURIComponent(end)}&offset=${offset}&limit=${limit}`,
    ),
  playback: () => apiFetch<ServerStatus['playback']>('/playback'),
  live: () => apiFetch<ServerStatus['playback']>('/playback/live', { method: 'POST' }),
  pause: () => apiFetch<ServerStatus['playback']>('/playback/pause', { method: 'POST' }),
  play: (speed: number) =>
    apiFetch<ServerStatus['playback']>('/playback/play', {
      method: 'POST',
      body: JSON.stringify({ speed }),
    }),
  seek: (timestamp: number) =>
    apiFetch<ServerStatus['playback']>('/playback/seek', {
      method: 'POST',
      body: JSON.stringify({ timestamp }),
    }),
  step: (unit: PlaybackStepUnit, direction: PlaybackStepDirection, count = 1) =>
    apiFetch<ServerStatus['playback']>('/playback/step', {
      method: 'POST',
      body: JSON.stringify({ unit, direction, count }),
    }),
  setSpeed: (speed: number) =>
    apiFetch<ServerStatus['playback']>('/playback/speed', {
      method: 'PUT',
      body: JSON.stringify({ speed }),
    }),
  clear: () =>
    apiFetch<{ cleared: boolean }>('/memory/clear', {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    }),
  saveSession: (name: string, overwrite = false) =>
    apiFetch<{ operation_id: string }>(`/sessions/${encodeURIComponent(name)}/save`, {
      method: 'POST',
      body: JSON.stringify({ overwrite }),
    }),
  loadSession: (name: string) =>
    apiFetch<{ operation_id: string }>(`/sessions/${encodeURIComponent(name)}/load`, {
      method: 'POST',
    }),
  operation: (id: string) =>
    apiFetch<{ operation: OperationRecord }>(`/operations/${encodeURIComponent(id)}`),
  seriesCatalog: (aircraftId: string) =>
    apiFetch<{ aircraft_id: string; fields: SeriesCatalogItem[] }>(
      `/aircraft/${encodeURIComponent(aircraftId)}/series/catalog`,
    ),
  querySeries: (
    selections: SeriesSelection[],
    timeRange: { start: number; end: number } | null,
    maxPoints: number | null,
  ) =>
    apiFetch<{ series: SeriesData[] }>('/series/query', {
      method: 'POST',
      body: JSON.stringify({
        selections,
        time_range: timeRange,
        max_points: maxPoints,
      }),
    }),
  workspace: () => apiFetch<{ workspace: WorkspaceDocument | null }>('/workspace'),
  saveWorkspace: (workspace: WorkspaceSnapshot) =>
    apiFetch<{ workspace: WorkspaceDocument }>('/workspace', {
      method: 'PUT',
      body: JSON.stringify(workspace),
    }),
}

export function websocketUrl(): string {
  return resolveWebSocketUrl(runtimeConfig.websocket_url)
}
