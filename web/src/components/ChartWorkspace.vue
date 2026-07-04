<script setup lang="ts">
import { BarChart3, Layers3, Plus } from 'lucide-vue-next'
import { computed } from 'vue'
import { GridItem, GridLayout } from 'vue3-grid-layout-next'

import { useWorkspaceStore } from '@/stores/workspace'
import ChartCard from '@/components/ChartCard.vue'

const workspace = useWorkspaceStore()
const layout = computed(() =>
  workspace.workspace.charts.map((chart) => ({
    i: chart.id,
    x: chart.x,
    y: chart.y,
    w: chart.w,
    h: chart.h,
  })),
)
</script>

<template>
  <main class="relative min-h-0 min-w-0 flex-1 overflow-auto">
    <div
      v-if="workspace.workspace.basket.length"
      class="sticky top-0 z-20 m-3 flex items-center gap-2 rounded-lg border border-(--accent) bg-(--panel-bg) p-2 shadow-lg"
    >
      <Layers3 class="h-4 w-4 text-(--accent)" />
      <span class="text-xs">{{ workspace.workspace.basket.length }} selected</span>
      <span
        v-for="curve in workspace.workspace.basket"
        :key="`${curve.aircraft_id}:${curve.alias}`"
        class="rounded-full bg-(--accent-soft) px-2 py-1 text-[11px]"
      >
        {{ curve.alias }}
      </span>
      <div class="ml-auto flex gap-2">
        <button class="toolbar-button" @click="workspace.addBasketToSelected">Add to chart</button>
        <button class="toolbar-button" @click="workspace.createChart">
          <Plus class="h-3.5 w-3.5" />New chart
        </button>
      </div>
    </div>

    <div
      v-if="workspace.workspace.charts.length === 0"
      class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-(--text-muted)"
    >
      <BarChart3 class="h-12 w-12 opacity-50" />
      <p class="text-sm">Choose fields from the left panel, then create a chart.</p>
      <button class="toolbar-button" @click="workspace.createChart">
        <Plus class="h-4 w-4" />New chart
      </button>
    </div>

    <GridLayout
      v-else
      :layout="layout"
      :col-num="12"
      :row-height="72"
      :margin="[12, 12]"
      :is-draggable="true"
      :is-resizable="true"
      :vertical-compact="true"
      draggable-handle=".chart-drag-handle"
      @layout-updated="workspace.updateLayout"
    >
      <GridItem
        v-for="chart in workspace.workspace.charts"
        :key="chart.id"
        :i="chart.id"
        :x="chart.x"
        :y="chart.y"
        :w="chart.w"
        :h="chart.h"
        :min-w="3"
        :min-h="3"
      >
        <ChartCard :chart="chart" />
      </GridItem>
    </GridLayout>
  </main>
</template>
