<script setup lang="ts">
import { BarChart3, Trash2 } from 'lucide-vue-next'
import { computed, onMounted, watch } from 'vue'
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
const visibleCurves = computed(() => props.chart.curves.filter((curve) => curve.visible))
const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])
const timeOrigin = computed(() => bounds.value[0])
const palette = computed(() =>
  workspace.workspace.theme === 'dark'
    ? {
        text: '#bac4cf',
        muted: '#7f8c9a',
        border: '#697583',
        grid: 'rgba(127, 140, 154, 0.14)',
        panel: '#20262d',
        accentSoft: 'rgba(75, 146, 240, 0.18)',
        cursor: '#f59e0b',
      }
    : {
        text: '#465465',
        muted: '#748092',
        border: '#7b8794',
        grid: 'rgba(70, 84, 101, 0.13)',
        panel: '#edf2f7',
        accentSoft: 'rgba(36, 120, 223, 0.13)',
        cursor: '#c56700',
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
    view.zoom_start_value === undefined
      ? undefined
      : toRelativeTime(view.zoom_start_value, timeOrigin.value)
  const zoomEndValue =
    view.zoom_end_value === undefined
      ? undefined
      : toRelativeTime(view.zoom_end_value, timeOrigin.value)
  return {
    animation: false,
    backgroundColor: 'transparent',
    grid: { left: 54, right: 54, top: props.chart.legend_visible ? 48 : 22, bottom: 54 },
    legend: {
      show: props.chart.legend_visible,
      textStyle: { color: palette.value.text, fontSize: 11 },
      top: 8,
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
          const curve = visibleCurves.value[item.seriesIndex]
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
      min: 0,
      max: Math.max(bounds.value[1] - bounds.value[0], 0.001),
      axisLine: { lineStyle: { color: palette.value.border } },
      axisLabel: {
        color: palette.value.muted,
        formatter: (value: number) => formatRelativeTime(value, false),
      },
      splitLine: { lineStyle: { color: palette.value.grid } },
    },
    yAxis: [
      {
        type: 'value',
        scale: true,
        position: 'left',
        axisLabel: { color: palette.value.muted },
        splitLine: { lineStyle: { color: palette.value.grid } },
      },
      {
        type: 'value',
        scale: true,
        position: 'right',
        axisLabel: { color: palette.value.muted },
        splitLine: { show: false },
      },
    ],
    dataZoom: [
      {
        type: 'inside',
        filterMode: 'none',
        start: view.zoom_start,
        end: view.zoom_end,
        startValue: zoomStartValue,
        endValue: zoomEndValue,
      },
      {
        type: 'slider',
        filterMode: 'none',
        start: view.zoom_start,
        end: view.zoom_end,
        startValue: zoomStartValue,
        endValue: zoomEndValue,
        height: 18,
        bottom: 8,
        borderColor: palette.value.border,
        backgroundColor: palette.value.panel,
        fillerColor: palette.value.accentSoft,
      },
    ],
    series: visibleCurves.value.map((curve, index) => ({
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
    props.chart.curves,
    workspace.workspace.query_start !== null && workspace.workspace.query_end !== null
      ? { start: workspace.workspace.query_start, end: workspace.workspace.query_end }
      : null,
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

onMounted(() => void load())
watch(
  () => [
    props.chart.curves.map(curveKey).join('|'),
    workspace.workspace.query_start,
    workspace.workspace.query_end,
    workspace.workspace.max_points,
    server.storeRevision,
  ],
  () => void load(),
)
</script>

<template>
  <article
    class="panel-surface flex h-full min-h-0 flex-col overflow-hidden rounded-lg"
    :class="workspace.workspace.selected_chart_id === chart.id ? 'ring-1 ring-(--accent)' : ''"
    @pointerdown="workspace.workspace.selected_chart_id = chart.id"
  >
    <header
      class="chart-drag-handle flex h-9 shrink-0 cursor-move items-center gap-2 border-b border-(--border-color) px-3"
    >
      <BarChart3 class="h-4 w-4 text-(--accent)" />
      <span class="min-w-0 flex-1 truncate text-xs font-semibold">{{ chart.title }}</span>
      <span class="text-[10px] text-(--text-muted)">{{ chart.curves.length }} curves</span>
      <button
        class="icon-button h-7 w-7"
        title="Remove chart"
        @click.stop="workspace.removeChart(chart.id)"
      >
        <Trash2 class="h-3.5 w-3.5" />
      </button>
    </header>
    <VChart
      class="min-h-0 flex-1"
      :option="option"
      :update-options="{ notMerge: false, lazyUpdate: true }"
      autoresize
      @click="seekFromChart"
      @datazoom="updateZoom"
    />
  </article>
</template>
