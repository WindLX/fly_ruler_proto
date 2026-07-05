export type PlaybackMode = 'live' | 'replay_paused' | 'replay_playing'
export type ThemeMode = 'dark' | 'light'
export type LinePattern = 'solid' | 'dashed' | 'dotted'
export type ValueFormat = 'auto' | 'fixed' | 'scientific'

export interface PlaybackState {
  mode: PlaybackMode
  cursor_secs: number | null
  speed: number
  bounds: [number, number] | null
  revision: number
}

export interface StoreStats {
  aircraft_count: number
  state_count: number
  event_count: number
  time_bounds: [number, number] | null
}

export interface ServerStatus {
  protocol_version: string
  api_version: string
  store: StoreStats
  playback: PlaybackState
  ingestion: { enabled: boolean; dropped_during_maintenance: number }
  persistence_operation_active: boolean
  udp_sessions: Array<{
    addr: string
    client_uuid_hex: string
    last_seen_secs: number
  }>
}

export interface AircraftSummary {
  id: string
  name: string | null
  toml_config: string | null
  time_range: [number, number] | null
  state_count: number
  event_count: number
  spawned_at_cursor: boolean
}

export interface AircraftState {
  position?: Vector3 | null
  velocity?: Vector3 | null
  attitude?: Quaternion | null
  angular_velocity?: Vector3 | null
  derived?: Record<string, number | null> | null
  control_surfaces?: Record<string, number | null> | null
  engines: Array<{ index: number; throttle_lever_ratio: number | null }>
  custom_fields: Record<string, { kind: string; value: unknown }>
}

export interface Vector3 {
  x: number
  y: number
  z: number
}

export interface Quaternion {
  w: number
  x: number
  y: number
  z: number
}

export interface TimestampedState {
  timestamp_secs: number
  state: AircraftState
}

export interface AircraftEvent {
  aircraft_id?: string
  timestamp_secs: number
  event_type: 'spawn' | 'despawn' | 'custom'
  name?: string
  reason?: string | null
}

export type SeriesSelector =
  | { kind: 'standard'; path: string }
  | { kind: 'engine_throttle'; index: number }
  | { kind: 'custom'; field_id: string }

export interface SeriesCatalogItem {
  selector: SeriesSelector
  group: string
  label: string
  unit: string | null
}

export interface SeriesSelection {
  aircraft_id: string
  selector: SeriesSelector
}

export interface SeriesData {
  key: string
  aircraft_id: string
  selector: SeriesSelector
  points: Array<[number, number]>
  total_points: number
  returned_points: number
  stats?: { min: number; max: number; last: number; start: number; end: number } | null
}

export interface CurveStyle extends SeriesSelection {
  alias: string
  color: string
  line_pattern: LinePattern
  line_width: number
  opacity: number
  visible: boolean
  y_axis: 'left' | 'right'
  smooth: boolean
  show_symbol: boolean
  scale: number
  offset: number
  unit: string | null
  value_format: ValueFormat
  precision: number
}

export interface ChartModel {
  id: string
  title: string
  x: number
  y: number
  w: number
  h: number
  legend_visible: boolean
  curves: CurveStyle[]
  view: {
    zoom_start?: number
    zoom_end?: number
    zoom_start_value?: number
    zoom_end_value?: number
  }
}

export interface WorkspaceSnapshot {
  version: 1
  theme: ThemeMode
  locale: string
  left_panel_open: boolean
  right_panel_open: boolean
  basket_open: boolean
  selected_aircraft_id: string | null
  selected_chart_id: string | null
  sync_charts: boolean
  query_start: number | null
  query_end: number | null
  max_points: number
  charts: ChartModel[]
  basket: CurveStyle[]
}

export interface WorkspaceDocument {
  revision: number
  updated_at_secs: number
  workspace: WorkspaceSnapshot
}

export interface SessionSummary {
  name: string
  metadata: Record<string, unknown> | null
}

export interface OperationRecord {
  id: string
  kind: string
  session: string
  state: 'queued' | 'running' | 'succeeded' | 'failed'
  error: string | null
}

export interface SnapshotMessage {
  type: 'snapshot'
  sequence: number
  server_time_secs: number
  playback: PlaybackState
  store: StoreStats
  aircraft: Record<string, { spawned: boolean; sample: TimestampedState }>
  truncated: boolean
}
