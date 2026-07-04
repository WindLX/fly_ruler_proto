import type {
  AircraftState,
  CurveStyle,
  SeriesCatalogItem,
  SeriesSelector,
  WorkspaceSnapshot,
} from '@/types'

const colors = ['#4b92f0', '#38a169', '#d69e2e', '#e35d6a', '#8b5cf6', '#06b6d4']

export function selectorKey(selector: SeriesSelector): string {
  if (selector.kind === 'standard') return `standard:${selector.path}`
  if (selector.kind === 'engine_throttle') return `engine_throttle:${selector.index}`
  return `custom:${selector.field_id}`
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
    theme: 'dark',
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

export function extractValue(state: AircraftState, selector: SeriesSelector): number | null {
  if (selector.kind === 'engine_throttle') {
    return (
      state.engines.find((engine) => engine.index === selector.index)?.throttle_lever_ratio ?? null
    )
  }
  if (selector.kind === 'custom') {
    const value = state.custom_fields[selector.field_id]?.value
    if (typeof value === 'number') return value
    if (typeof value === 'boolean') return value ? 1 : 0
    return null
  }
  const segments = selector.path.split('.')
  let current: unknown = state
  for (const segment of segments) {
    if (!current || typeof current !== 'object') return null
    current = (current as Record<string, unknown>)[segment]
  }
  return typeof current === 'number' && Number.isFinite(current) ? current : null
}
