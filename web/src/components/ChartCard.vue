<script setup lang="ts">
import { BarChart3, Radio, Trash2 } from 'lucide-vue-next'
import { computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart } from 'echarts/charts'
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  TitleComponent,
  TooltipComponent,
} from 'echarts/components'
import type { EChartsOption } from 'echarts'

import { useSeriesStore } from '@/stores/series'
import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import type { ChartModel, CurveStyle } from '@/types'
import {
  curveKey,
  effectiveTimeRange,
  formatAbsoluteTime,
  formatNumber,
  formatRelativeTime,
  toAbsoluteTime,
  toRelativeTime,
} from '@/utils'

use([
  CanvasRenderer,
  LineChart,
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  TitleComponent,
  TooltipComponent,
])

const props = defineProps<{ chart: ChartModel }>()
const seriesStore = useSeriesStore()
const server = useServerStore()
const workspace = useWorkspaceStore()
const { t } = useI18n()
const visibleCurves = computed(() => props.chart.curves.filter((curve) => curve.visible))
const aircraftIds = computed(() => new Set(server.availableAircraftIds))
const queryableCurves = computed(() =>
  props.chart.curves.filter((curve) => aircraftIds.value.has(curve.aircraft_id)),
)
const renderedCurves = computed(() =>
  visibleCurves.value.filter((curve) => aircraftIds.value.has(curve.aircraft_id)),
)
const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])
const queryRange = computed(() =>
  effectiveTimeRange(
    workspace.workspace.query_start,
    workspace.workspace.query_end,
    server.playback?.bounds ?? null,
  ),
)
const displayRange = computed<[number, number]>(() => {
  const range = queryRange.value
  return range ? [range.start, range.end] : bounds.value
})
const timeOrigin = computed(() => bounds.value[0])
const pointCount = computed(() =>
  renderedCurves.value.reduce(
    (total, curve) => total + (seriesStore.data[curveKey(curve)]?.returned_points ?? 0),
    0,
  ),
)
const unavailableCount = computed(
  () => props.chart.curves.filter((curve) => !aircraftIds.value.has(curve.aircraft_id)).length,
)
const loading = computed(() => seriesStore.isLoading(queryableCurves.value))
const blockingLoading = computed(() => loading.value && pointCount.value === 0)
const loadError = computed(() => seriesStore.errorFor(queryableCurves.value))
const hasManualZoom = computed(
  () =>
    typeof props.chart.view.zoom_start === 'number' ||
    typeof props.chart.view.zoom_end === 'number' ||
    typeof props.chart.view.zoom_start_value === 'number' ||
    typeof props.chart.view.zoom_end_value === 'number',
)
const palette = computed(() =>
  workspace.workspace.theme === 'dark'
    ? {
        text: '#b8b8b8',
        muted: '#8b8b8b',
        border: '#606060',
        grid: 'rgba(255, 255, 255, 0.07)',
        panel: '#252525',
        accentSoft: 'rgba(86, 128, 194, 0.24)',
        cursor: '#f19a3e',
      }
    : {
        text: '#444444',
        muted: '#707070',
        border: '#777777',
        grid: 'rgba(0, 0, 0, 0.09)',
        panel: '#d6d6d6',
        accentSoft: 'rgba(71, 119, 189, 0.18)',
        cursor: '#cc6810',
      },
)

function transformed(curve: CurveStyle): Array<[number, number]> {
  return (seriesStore.data[curveKey(curve)]?.points ?? []).map(([time, value]) => [
    toRelativeTime(time, timeOrigin.value),
    value * curve.scale + curve.offset,
  ])
}

const option = computed<EChartsOption>(() => {
  const cursor = server.playback?.cursor_secs
  const view = props.chart.view
  const zoomStartValue =
    typeof view.zoom_start_value !== 'number'
      ? undefined
      : toRelativeTime(view.zoom_start_value, timeOrigin.value)
  const zoomEndValue =
    typeof view.zoom_end_value !== 'number'
      ? undefined
      : toRelativeTime(view.zoom_end_value, timeOrigin.value)
  const hasAbsoluteZoom = zoomStartValue !== undefined || zoomEndValue !== undefined
  const xMin = toRelativeTime(displayRange.value[0], timeOrigin.value)
  const xMax = Math.max(toRelativeTime(displayRange.value[1], timeOrigin.value), xMin + 0.001)
  return {
    animation: false,
    backgroundColor: 'transparent',
    grid: { left: 48, right: 48, top: props.chart.legend_visible ? 38 : 16, bottom: 45 },
    legend: {
      show: props.chart.legend_visible,
      textStyle: { color: palette.value.text, fontSize: 10 },
      top: 4,
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'cross' },
      formatter(params: unknown) {
        const items = Array.isArray(params) ? params : [params]
        const first = items[0] as { axisValue?: number } | undefined
        const relative = Number(first?.axisValue ?? 0)
        const absolute = toAbsoluteTime(relative, timeOrigin.value)
        const lines = [
          `<b>${formatRelativeTime(relative)}</b>`,
          formatAbsoluteTime(absolute, workspace.workspace.locale),
        ]
        for (const item of items as Array<{
          seriesIndex: number
          marker: string
          seriesName: string
          value: [number, number]
        }>) {
          const curve = renderedCurves.value[item.seriesIndex]
          if (!curve) continue
          lines.push(
            `${item.marker}${item.seriesName}: ${formatNumber(item.value[1], curve.value_format, curve.precision)} ${curve.unit ?? ''}`,
          )
        }
        return lines.join('<br/>')
      },
    },
    xAxis: {
      type: 'value',
      scale: true,
      name: 'Δt',
      min: xMin,
      max: xMax,
      axisLine: { lineStyle: { color: palette.value.border } },
      axisLabel: {
        color: palette.value.muted,
        fontSize: 10,
        formatter: (value: number) => formatRelativeTime(value, false),
      },
      splitLine: { lineStyle: { color: palette.value.grid } },
    },
    yAxis: [
      {
        type: 'value',
        scale: true,
        position: 'left',
        axisLabel: { color: palette.value.muted, fontSize: 10 },
        splitLine: { lineStyle: { color: palette.value.grid } },
      },
      {
        type: 'value',
        scale: true,
        position: 'right',
        axisLabel: { color: palette.value.muted, fontSize: 10 },
        splitLine: { show: false },
      },
    ],
    dataZoom: [
      {
        type: 'inside',
        filterMode: 'none',
        start:
          !hasAbsoluteZoom && typeof view.zoom_start === 'number' ? view.zoom_start : undefined,
        end: !hasAbsoluteZoom && typeof view.zoom_end === 'number' ? view.zoom_end : undefined,
        startValue: zoomStartValue,
        endValue: zoomEndValue,
      },
      {
        type: 'slider',
        filterMode: 'none',
        start:
          !hasAbsoluteZoom && typeof view.zoom_start === 'number' ? view.zoom_start : undefined,
        end: !hasAbsoluteZoom && typeof view.zoom_end === 'number' ? view.zoom_end : undefined,
        startValue: zoomStartValue,
        endValue: zoomEndValue,
        height: 14,
        bottom: 5,
        borderColor: palette.value.border,
        backgroundColor: palette.value.panel,
        fillerColor: palette.value.accentSoft,
      },
    ],
    series: renderedCurves.value.map((curve, index) => ({
      name: curve.alias,
      type: 'line',
      data: transformed(curve),
      yAxisIndex: curve.y_axis === 'right' ? 1 : 0,
      showSymbol: curve.show_symbol,
      symbolSize: 5,
      smooth: curve.smooth,
      lineStyle: {
        color: curve.color,
        width: curve.line_width,
        opacity: curve.opacity,
        type: curve.line_pattern,
      },
      itemStyle: { color: curve.color, opacity: curve.opacity },
      markLine:
        index === 0 && cursor !== null && cursor !== undefined
          ? {
              silent: true,
              symbol: 'none',
              label: { show: false },
              lineStyle: { color: palette.value.cursor, width: 1 },
              data: [{ xAxis: toRelativeTime(cursor, timeOrigin.value) }],
            }
          : undefined,
    })),
  }
})

async function load() {
  await seriesStore.loadCurves(
    queryableCurves.value,
    queryRange.value,
    workspace.workspace.max_points,
  )
}

function seekFromChart(params: { value?: unknown }) {
  const relative = Array.isArray(params.value) ? params.value[0] : null
  if (typeof relative === 'number') void server.seek(toAbsoluteTime(relative, timeOrigin.value))
}

function updateZoom(params: unknown) {
  const payload = params as {
    start?: number
    end?: number
    startValue?: number
    endValue?: number
    batch?: Array<{ start?: number; end?: number; startValue?: number; endValue?: number }>
  }
  const zoom = payload.batch?.[0] ?? payload
  workspace.updateChartView(props.chart.id, {
    zoom_start: zoom.start,
    zoom_end: zoom.end,
    zoom_start_value:
      typeof zoom.startValue === 'number'
        ? toAbsoluteTime(zoom.startValue, timeOrigin.value)
        : undefined,
    zoom_end_value:
      typeof zoom.endValue === 'number'
        ? toAbsoluteTime(zoom.endValue, timeOrigin.value)
        : undefined,
  })
}

function resetZoom() {
  workspace.updateChartView(props.chart.id, {}, false)
}

onMounted(() => void load())
watch(
  () => [
    props.chart.curves.map(curveKey).join('|'),
    workspace.workspace.query_start,
    workspace.workspace.query_end,
    workspace.workspace.max_points,
    server.storeRevision,
    server.availableAircraftIds.join('|'),
  ],
  () => void load(),
)
</script>

<template>
  <article
    class="chart-card"
    :class="{ 'chart-card-selected': workspace.workspace.selected_chart_id === chart.id }"
    @pointerdown="workspace.workspace.selected_chart_id = chart.id"
  >
    <header class="chart-drag-handle">
      <BarChart3 class="h-4 w-4 text-(--accent)" />
      <span class="min-w-0 flex-1 truncate text-xs font-semibold">{{ chart.title }}</span>
      <span class="editor-stat">{{ t('chart.curveCount', { count: chart.curves.length }) }}</span>
      <span class="editor-stat">{{ t('chart.pointCount', { count: pointCount }) }}</span>
      <button
        v-if="server.playback?.mode === 'live' && hasManualZoom"
        class="editor-button chart-live-button"
        :title="t('chart.returnLive')"
        @click.stop="resetZoom"
      >
        <Radio class="h-3.5 w-3.5" />{{ t('chart.liveFollow') }}
      </button>
      <button
        class="editor-icon-button"
        :title="t('chart.remove')"
        @click.stop="workspace.removeChart(chart.id)"
      >
        <Trash2 class="h-3.5 w-3.5" />
      </button>
    </header>
    <div class="relative min-h-0 flex-1">
      <VChart
        class="absolute inset-0"
        :option="option"
        :update-options="{ notMerge: false, lazyUpdate: true }"
        autoresize
        @click="seekFromChart"
        @datazoom="updateZoom"
      />
      <div v-if="blockingLoading" class="chart-overlay">
        <span class="loading-spinner" />
        <span>{{ t('chart.loading') }}</span>
      </div>
      <div v-else-if="loadError" class="chart-overlay chart-overlay-error">
        <strong>{{ t('chart.loadFailed') }}</strong>
        <span>{{ loadError }}</span>
      </div>
      <div v-else-if="queryableCurves.length === 0" class="chart-overlay">
        <span>{{ t('chart.aircraftUnavailable') }}</span>
      </div>
      <div v-else-if="pointCount === 0" class="chart-overlay">
        <span>{{ t('chart.noPoints') }}</span>
        <span v-if="unavailableCount" class="text-[10px]">
          {{ t('chart.unavailableCurves', { count: unavailableCount }) }}
        </span>
      </div>
    </div>
  </article>
</template>
