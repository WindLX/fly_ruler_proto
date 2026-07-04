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
import { curveKey, formatNumber } from '@/utils'

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

function transformed(curve: CurveStyle): Array<[number, number]> {
  return (seriesStore.data[curveKey(curve)]?.points ?? []).map(([time, value]) => [
    time,
    value * curve.scale + curve.offset,
  ])
}

const option = computed<EChartsOption>(() => {
  const cursor = server.playback?.cursor_secs
  return {
    animation: false,
    backgroundColor: 'transparent',
    grid: { left: 54, right: 54, top: props.chart.legend_visible ? 48 : 22, bottom: 54 },
    legend: {
      show: props.chart.legend_visible,
      textStyle: { color: 'var(--text-secondary)', fontSize: 11 },
      top: 8,
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'cross' },
      formatter(params: unknown) {
        const items = Array.isArray(params) ? params : [params]
        const first = items[0] as { axisValue?: number } | undefined
        const lines = [`<b>${Number(first?.axisValue ?? 0).toFixed(3)} s</b>`]
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
      name: 't [s]',
      axisLine: { lineStyle: { color: 'var(--border-strong)' } },
      axisLabel: { color: 'var(--text-muted)' },
      splitLine: { lineStyle: { color: 'var(--grid-color)' } },
    },
    yAxis: [
      {
        type: 'value',
        position: 'left',
        axisLabel: { color: 'var(--text-muted)' },
        splitLine: { lineStyle: { color: 'var(--grid-color)' } },
      },
      {
        type: 'value',
        position: 'right',
        axisLabel: { color: 'var(--text-muted)' },
        splitLine: { show: false },
      },
    ],
    dataZoom: [
      { type: 'inside', filterMode: 'none' },
      {
        type: 'slider',
        height: 18,
        bottom: 8,
        borderColor: 'var(--border-color)',
        backgroundColor: 'var(--panel-muted)',
        fillerColor: 'var(--accent-soft)',
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
      sampling: 'lttb',
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
              lineStyle: { color: 'var(--cursor-color)', width: 1 },
              data: [{ xAxis: cursor }],
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
  const timestamp = Array.isArray(params.value) ? params.value[0] : null
  if (typeof timestamp === 'number') void server.seek(timestamp)
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
    <VChart class="min-h-0 flex-1" :option="option" autoresize @click="seekFromChart" />
  </article>
</template>
