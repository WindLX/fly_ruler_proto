<script setup lang="ts">
import { BarChart3, Layers3, Plus } from 'lucide-vue-next'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { GridItem, GridLayout } from 'vue3-grid-layout-next'

import { useWorkspaceStore } from '@/stores/workspace'
import ChartCard from '@/components/ChartCard.vue'

const workspace = useWorkspaceStore()
const { t } = useI18n()
type LayoutItem = { i: string; x: number; y: number; w: number; h: number }
const layout = ref<LayoutItem[]>([])
const chartById = computed(
  () => new Map(workspace.workspace.charts.map((chart) => [chart.id, chart])),
)

watch(
  () =>
    workspace.workspace.charts.map((chart) => ({
      i: chart.id,
      x: chart.x,
      y: chart.y,
      w: chart.w,
      h: chart.h,
    })),
  (next) => {
    if (!sameLayout(layout.value, next)) layout.value = next
  },
  { deep: true, immediate: true },
)

function commitLayout(next: LayoutItem[]) {
  if (!sameLayout(next, workspace.workspace.charts)) {
    workspace.updateLayout(next)
  }
}

function nextChartTitle(): string {
  return t('chart.defaultTitle', { number: workspace.workspace.charts.length + 1 })
}

function sameLayout(
  left: Array<{ i?: string; id?: string; x: number; y: number; w: number; h: number }>,
  right: Array<{ i?: string; id?: string; x: number; y: number; w: number; h: number }>,
): boolean {
  if (left.length !== right.length) return false
  return left.every((item, index) => {
    const other = right[index]
    return (
      !!other &&
      (item.i ?? item.id) === (other.i ?? other.id) &&
      item.x === other.x &&
      item.y === other.y &&
      item.w === other.w &&
      item.h === other.h
    )
  })
}
</script>

<template>
  <main class="relative min-h-0 min-w-0 flex-1 overflow-auto">
    <div v-if="workspace.workspace.basket.length" class="basket-bar">
      <Layers3 class="h-4 w-4 text-(--accent)" />
      <span class="text-xs font-semibold">
        {{ t('chart.selected', { count: workspace.workspace.basket.length }) }}
      </span>
      <span
        v-for="curve in workspace.workspace.basket"
        :key="`${curve.aircraft_id}:${curve.alias}`"
        class="rounded-full bg-(--accent-soft) px-2 py-1 text-[11px]"
      >
        {{ curve.alias }}
      </span>
      <div class="ml-auto flex gap-2">
        <button class="toolbar-button" @click="workspace.addBasketToSelected(nextChartTitle())">
          {{ t('chart.addToChart') }}
        </button>
        <button class="toolbar-button" @click="workspace.createChart(nextChartTitle())">
          <Plus class="h-3.5 w-3.5" />{{ t('chart.new') }}
        </button>
      </div>
    </div>

    <div
      v-if="workspace.workspace.charts.length === 0"
      class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-(--text-muted)"
    >
      <BarChart3 class="h-12 w-12 opacity-50" />
      <p class="text-sm">{{ t('chart.emptyHint') }}</p>
      <button class="toolbar-button" @click="workspace.createChart(nextChartTitle())">
        <Plus class="h-4 w-4" />{{ t('chart.new') }}
      </button>
    </div>

    <GridLayout
      v-else
      v-model:layout="layout"
      :col-num="12"
      :row-height="72"
      :margin="[12, 12]"
      :is-draggable="true"
      :is-resizable="true"
      :vertical-compact="true"
      draggable-handle=".chart-drag-handle"
      @layout-updated="commitLayout"
    >
      <GridItem
        v-for="item in layout"
        :key="item.i"
        :i="item.i"
        :x="item.x"
        :y="item.y"
        :w="item.w"
        :h="item.h"
        :min-w="3"
        :min-h="3"
      >
        <ChartCard v-if="chartById.get(item.i)" :chart="chartById.get(item.i)!" />
      </GridItem>
    </GridLayout>
  </main>
</template>
