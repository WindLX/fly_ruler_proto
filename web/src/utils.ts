import type {
  AircraftState,
  ChartModel,
  CurveStyle,
  SeriesCatalogItem,
  SeriesSelector,
  WorkspaceSnapshot,
} from '@/types'

const colors = ['#4b92f0', '#38a169', '#d69e2e', '#e35d6a', '#8b5cf6', '#06b6d4']
const UNIX_TIMESTAMP_THRESHOLD = 946_684_800

export function selectorKey(selector: SeriesSelector): string {
  if (selector.kind === 'standard') return `standard:${selector.path}`
  if (selector.kind === 'propulsor') {
    return `propulsor:${selector.propulsor_id}:${selector.field}`
  }
  return `telemetry:${selector.stream_id}:${selector.field_id}`
}

export function curveKey(curve: { aircraft_id: string; selector: SeriesSelector }): string {
  return `${curve.aircraft_id}::${selectorKey(curve.selector)}`
}

export function createCurveStyle(
  aircraftId: string,
  field: SeriesCatalogItem,
  index = 0,
): CurveStyle {
  return {
    aircraft_id: aircraftId,
    selector: field.selector,
    alias: field.label,
    color: colors[index % colors.length] ?? '#4b92f0',
    line_pattern: 'solid',
    line_width: 2,
    opacity: 1,
    visible: true,
    y_axis: 'left',
    smooth: false,
    show_symbol: false,
    scale: 1,
    offset: 0,
    unit: field.unit,
    value_format: 'auto',
    precision: 3,
  }
}

export function defaultWorkspace(): WorkspaceSnapshot {
  return {
    version: 1,
    theme: 'light',
    locale: 'zh-CN',
    left_panel_open: true,
    right_panel_open: true,
    basket_open: true,
    selected_aircraft_id: null,
    selected_chart_id: null,
    sync_charts: true,
    query_start: null,
    query_end: null,
    max_points: 2000,
    charts: [],
    basket: [],
  }
}

export function normalizeWorkspace(workspace: WorkspaceSnapshot): WorkspaceSnapshot {
  const normalized = JSON.parse(JSON.stringify(workspace)) as WorkspaceSnapshot
  normalized.query_start = finiteOrNull(normalized.query_start)
  normalized.query_end = finiteOrNull(normalized.query_end)
  if (
    normalized.query_start !== null &&
    normalized.query_end !== null &&
    normalized.query_start > normalized.query_end
  ) {
    normalized.query_start = null
    normalized.query_end = null
  }
  for (const chart of normalized.charts) {
    chart.view = normalizeChartView(chart.view)
  }
  return normalized
}

export function normalizeChartView(
  view: ChartModel['view'] | null | undefined,
): ChartModel['view'] {
  if (!view) return {}
  return Object.fromEntries(
    Object.entries(view).filter((entry): entry is [string, number] => Number.isFinite(entry[1])),
  ) as ChartModel['view']
}

export function effectiveTimeRange(
  start: number | null,
  end: number | null,
  bounds: [number, number] | null,
): { start: number; end: number } | null {
  if (
    start === null ||
    end === null ||
    !Number.isFinite(start) ||
    !Number.isFinite(end) ||
    start > end ||
    !bounds
  ) {
    return null
  }
  if (end < bounds[0] || start > bounds[1]) return null
  return {
    start: Math.max(start, bounds[0]),
    end: Math.min(end, bounds[1]),
  }
}

function finiteOrNull(value: number | null): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

export function formatNumber(
  value: number,
  format: CurveStyle['value_format'] = 'auto',
  precision = 3,
): string {
  if (!Number.isFinite(value)) return '—'
  if (format === 'fixed') return value.toFixed(precision)
  if (format === 'scientific') return value.toExponential(precision)
  return new Intl.NumberFormat(undefined, { maximumSignificantDigits: 7 }).format(value)
}

export function toRelativeTime(timestamp: number, origin: number): number {
  return timestamp - origin
}

export function toAbsoluteTime(relative: number, origin: number): number {
  return relative + origin
}

export function isUnixTimestamp(timestamp: number): boolean {
  return Number.isFinite(timestamp) && timestamp >= UNIX_TIMESTAMP_THRESHOLD
}

export function formatRelativeTime(seconds: number, showMilliseconds = true): string {
  if (!Number.isFinite(seconds)) return '—'
  const sign = seconds < 0 ? '−' : '+'
  let remaining = Math.abs(seconds)
  const days = Math.floor(remaining / 86_400)
  remaining -= days * 86_400
  const hours = Math.floor(remaining / 3_600)
  remaining -= hours * 3_600
  const minutes = Math.floor(remaining / 60)
  remaining -= minutes * 60
  const secondText = showMilliseconds
    ? remaining.toFixed(3).padStart(6, '0')
    : Math.floor(remaining).toString().padStart(2, '0')
  if (days > 0) {
    return `${sign}${days}d ${hours.toString().padStart(2, '0')}:${minutes
      .toString()
      .padStart(2, '0')}:${secondText}`
  }
  if (hours > 0) {
    return `${sign}${hours.toString().padStart(2, '0')}:${minutes
      .toString()
      .padStart(2, '0')}:${secondText}`
  }
  return `${sign}${minutes.toString().padStart(2, '0')}:${secondText}`
}

export function formatAbsoluteTime(timestamp: number, locale?: string): string {
  if (!isUnixTimestamp(timestamp)) return `t=${formatRelativeTime(timestamp, true).slice(1)}`
  const date = new Date(timestamp * 1000)
  const milliseconds = date.getMilliseconds().toString().padStart(3, '0')
  const formatted = new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(date)
  return `${formatted}.${milliseconds}`
}

export function niceTickStep(span: number, targetTicks = 8): number {
  if (!Number.isFinite(span) || span <= 0) return 1
  const raw = span / Math.max(targetTicks, 1)
  const magnitude = 10 ** Math.floor(Math.log10(raw))
  const normalized = raw / magnitude
  const nice = normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10
  return nice * magnitude
}

export interface TimelineTick {
  value: number
  level: 'major' | 'minor'
  label: string | null
}

const TIMELINE_STEPS = [
  0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900,
  1_800, 3_600, 7_200, 10_800, 21_600, 43_200, 86_400,
]

export function timelineTickStep(span: number, width: number, minLabelSpacing = 84): number {
  if (!Number.isFinite(span) || span <= 0 || !Number.isFinite(width) || width <= 0) return 1
  const target = Math.max(2, Math.ceil(width / minLabelSpacing))
  const raw = span / target
  const fixed = TIMELINE_STEPS.find((step) => step >= raw)
  if (fixed !== undefined) return fixed
  const days = raw / 86_400
  return niceTickStep(days, 1) * 86_400
}

export function generateTimelineTicks(
  span: number,
  width: number,
  minLabelSpacing = 84,
  offset = 0,
): TimelineTick[] {
  if (!Number.isFinite(span) || span <= 0 || !Number.isFinite(width) || width <= 0) return []
  const safeOffset = Number.isFinite(offset) ? offset : 0
  const major = timelineTickStep(span, width, minLabelSpacing)
  const divisions = minorTickDivisions(major)
  const minor = major / divisions
  const ticks: TimelineTick[] = []
  const epsilon = minor * 1e-6
  const firstIndex = Math.ceil((safeOffset - epsilon) / minor)
  const lastIndex = Math.floor((safeOffset + span + epsilon) / minor)
  const count = Math.min(2_000, Math.max(0, lastIndex - firstIndex + 1))
  for (let offsetIndex = 0; offsetIndex < count; offsetIndex++) {
    const index = firstIndex + offsetIndex
    const value = index * minor
    const localValue = value - safeOffset
    const isMajor = index % divisions === 0
    const pixel = (localValue / span) * width
    const showLabel = isMajor && pixel >= 34 && pixel <= width - 34
    ticks.push({
      value,
      level: isMajor ? 'major' : 'minor',
      label: showLabel ? formatRelativeTime(value, major < 1) : null,
    })
  }
  return ticks
}

export function eventFrameWindowSecs(
  bounds: [number, number] | null,
  approximateWidth = 1_000,
): number {
  if (!bounds) return 1e-6
  const span = Math.max(bounds[1] - bounds[0], 0)
  if (!Number.isFinite(span) || span <= 0) return 1e-6
  // Match the visual notion of a timeline cluster closely enough that
  // multi-aircraft events rendered as one marker are also inspected together.
  // The cap prevents long sessions from turning "current frame" into a broad
  // search window.
  return Math.max(1e-6, Math.min(0.5, span / Math.max(approximateWidth / 10, 1)))
}

export function zoomTimeRange(
  bounds: [number, number],
  view: [number, number],
  anchor: number,
  factor: number,
  minimumSpan = Math.max((bounds[1] - bounds[0]) / 10_000, 0.001),
): [number, number] {
  const fullSpan = bounds[1] - bounds[0]
  if (
    !Number.isFinite(fullSpan) ||
    fullSpan <= 0 ||
    !Number.isFinite(anchor) ||
    !Number.isFinite(factor) ||
    factor <= 0
  ) {
    return [...bounds]
  }

  const start = Math.max(bounds[0], Math.min(view[0], bounds[1]))
  const end = Math.max(start, Math.min(view[1], bounds[1]))
  const viewSpan = Math.max(end - start, Math.min(minimumSpan, fullSpan))
  const targetSpan = Math.min(
    fullSpan,
    Math.max(Math.min(minimumSpan, fullSpan), viewSpan * factor),
  )
  if (targetSpan >= fullSpan * (1 - 1e-9)) return [...bounds]

  const clampedAnchor = Math.min(end, Math.max(start, anchor))
  const anchorRatio = Math.min(1, Math.max(0, (clampedAnchor - start) / viewSpan))
  let nextStart = clampedAnchor - targetSpan * anchorRatio
  let nextEnd = nextStart + targetSpan
  if (nextStart < bounds[0]) {
    nextEnd += bounds[0] - nextStart
    nextStart = bounds[0]
  }
  if (nextEnd > bounds[1]) {
    nextStart -= nextEnd - bounds[1]
    nextEnd = bounds[1]
  }
  return [Math.max(bounds[0], nextStart), Math.min(bounds[1], nextEnd)]
}

function minorTickDivisions(step: number): number {
  const seconds = step < 1 ? step * 1_000 : step
  const normalized = seconds / 10 ** Math.floor(Math.log10(seconds))
  if (Math.abs(normalized - 2) < 1e-9) return 4
  if (Math.abs(normalized - 1.5) < 1e-9 || step === 15 || step === 900) return 3
  if (step === 30 || step === 60 || step === 1_800 || step === 3_600) return 6
  if (step >= 86_400) return 4
  return 5
}

export function extractValue(state: AircraftState, selector: SeriesSelector): number | null {
  if (selector.kind === 'telemetry') return null
  if (selector.kind === 'propulsor') {
    const value = state.propulsors?.find(
      (propulsor) => propulsor.propulsor_id === selector.propulsor_id,
    )?.[selector.field]
    return typeof value === 'number' && Number.isFinite(value) ? value : null
  }
  const segments = selector.path.split('.')
  let current: unknown = state
  for (const segment of segments) {
    if (!current || typeof current !== 'object') return null
    current = (current as Record<string, unknown>)[segment]
  }
  return typeof current === 'number' && Number.isFinite(current) ? current : null
}
