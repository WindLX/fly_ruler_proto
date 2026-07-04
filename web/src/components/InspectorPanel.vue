<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Eye, EyeOff, Trash2 } from 'lucide-vue-next'

import { api } from '@/api'
import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import type { AircraftEvent } from '@/types'

const server = useServerStore()
const workspace = useWorkspaceStore()
const chart = computed(() => workspace.selectedChart)
const events = ref<AircraftEvent[]>([])
const sample = computed(() => {
  const id = workspace.workspace.selected_aircraft_id
  return id ? server.samples[id] : null
})

watch(
  [() => workspace.workspace.selected_aircraft_id, () => server.storeRevision],
  async ([id]) => {
    events.value = id ? (await api.aircraftEvents(id)).items : []
  },
  { immediate: true },
)
</script>

<template>
  <aside
    class="panel-surface flex min-h-0 w-86 shrink-0 scrollbar-thin flex-col overflow-y-auto rounded-lg"
  >
    <section class="border-b border-(--border-color) p-3">
      <h2 class="tool-label">Current state</h2>
      <div v-if="sample" class="mt-2 space-y-2 text-xs">
        <div class="mono text-(--text-muted)">t = {{ sample.timestamp_secs.toFixed(6) }}</div>
        <pre class="state-json scrollbar-thin">{{ JSON.stringify(sample.state, null, 2) }}</pre>
      </div>
      <p v-else class="mt-2 text-xs text-(--text-muted)">No selected aircraft sample.</p>
    </section>

    <section class="border-b border-(--border-color) p-3">
      <h2 class="tool-label">Events</h2>
      <div v-if="events.length" class="mt-2 space-y-1">
        <div
          v-for="event in events"
          :key="`${event.timestamp_secs}:${event.event_type}`"
          class="flex gap-2 rounded border border-(--border-color) bg-(--card-bg) px-2 py-1.5 text-xs"
        >
          <span class="mono text-(--text-muted)">{{ event.timestamp_secs.toFixed(3) }}</span>
          <strong>{{ event.event_type }}</strong>
          <span class="min-w-0 truncate text-(--text-secondary)">{{
            event.name ?? event.reason ?? ''
          }}</span>
        </div>
      </div>
      <p v-else class="mt-2 text-xs text-(--text-muted)">No lifecycle events.</p>
    </section>

    <section v-if="chart" class="p-3">
      <h2 class="tool-label">Chart inspector</h2>
      <label class="field-label mt-3">
        Title
        <input v-model="chart.title" class="toolbar-input w-full" />
      </label>
      <label class="mt-2 flex items-center gap-2 text-xs">
        <input v-model="chart.legend_visible" type="checkbox" />Show legend
      </label>

      <article
        v-for="(curve, index) in chart.curves"
        :key="`${curve.aircraft_id}:${index}`"
        class="mt-3 rounded-lg border border-(--border-color) bg-(--card-bg) p-3"
      >
        <div class="flex items-center gap-2">
          <input
            v-model="curve.color"
            type="color"
            class="h-7 w-8 cursor-pointer border-0 bg-transparent"
          />
          <input v-model="curve.alias" class="toolbar-input min-w-0 flex-1" />
          <button class="icon-button h-7 w-7" @click="curve.visible = !curve.visible">
            <Eye v-if="curve.visible" class="h-3.5 w-3.5" />
            <EyeOff v-else class="h-3.5 w-3.5" />
          </button>
          <button class="icon-button h-7 w-7" @click="chart.curves.splice(index, 1)">
            <Trash2 class="h-3.5 w-3.5" />
          </button>
        </div>
        <div class="mt-3 grid grid-cols-2 gap-2">
          <label class="field-label"
            >Pattern
            <select v-model="curve.line_pattern" class="toolbar-select w-full">
              <option value="solid">Solid</option>
              <option value="dashed">Dashed</option>
              <option value="dotted">Dotted</option>
            </select>
          </label>
          <label class="field-label"
            >Y axis
            <select v-model="curve.y_axis" class="toolbar-select w-full">
              <option value="left">Left</option>
              <option value="right">Right</option>
            </select>
          </label>
          <label class="field-label"
            >Width
            <input
              v-model.number="curve.line_width"
              type="number"
              min="0.5"
              max="8"
              step="0.5"
              class="toolbar-input w-full"
            />
          </label>
          <label class="field-label"
            >Opacity
            <input
              v-model.number="curve.opacity"
              type="number"
              min="0"
              max="1"
              step="0.1"
              class="toolbar-input w-full"
            />
          </label>
          <label class="field-label"
            >Scale
            <input
              v-model.number="curve.scale"
              type="number"
              step="any"
              class="toolbar-input w-full"
            />
          </label>
          <label class="field-label"
            >Offset
            <input
              v-model.number="curve.offset"
              type="number"
              step="any"
              class="toolbar-input w-full"
            />
          </label>
          <label class="field-label"
            >Unit
            <input v-model="curve.unit" class="toolbar-input w-full" />
          </label>
          <label class="field-label"
            >Format
            <select v-model="curve.value_format" class="toolbar-select w-full">
              <option value="auto">Auto</option>
              <option value="fixed">Fixed</option>
              <option value="scientific">Scientific</option>
            </select>
          </label>
          <label class="field-label"
            >Precision
            <input
              v-model.number="curve.precision"
              type="number"
              min="0"
              max="8"
              class="toolbar-input w-full"
            />
          </label>
          <label class="mt-5 flex items-center gap-2 text-xs"
            ><input v-model="curve.smooth" type="checkbox" />Smooth</label
          >
          <label class="flex items-center gap-2 text-xs"
            ><input v-model="curve.show_symbol" type="checkbox" />Symbols</label
          >
        </div>
      </article>
    </section>
  </aside>
</template>
