<script setup lang="ts">
import { computed } from 'vue'
import { Eye, EyeOff, Trash2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import { eventFrameWindowSecs, formatAbsoluteTime, formatRelativeTime } from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const { t } = useI18n()
const chart = computed(() => workspace.selectedChart)
const currentFrameEvents = computed(() => {
  const cursor = server.playback?.cursor_secs
  if (cursor === null || cursor === undefined) return []
  const windowSecs = eventFrameWindowSecs(server.playback?.bounds ?? null)
  const nearest = server.timelineEvents.reduce<number | null>((best, event) => {
    const distance = Math.abs(event.timestamp_secs - cursor)
    if (distance > windowSecs) return best
    if (best === null || distance < Math.abs(best - cursor)) return event.timestamp_secs
    return best
  }, null)
  if (nearest === null) return []
  return server.timelineEvents
    .filter((event) => Math.abs(event.timestamp_secs - nearest) <= windowSecs)
    .sort((left, right) => {
      const timeOrder = left.timestamp_secs - right.timestamp_secs
      if (Math.abs(timeOrder) > 1e-9) return timeOrder
      return (left.aircraft_id ?? '').localeCompare(right.aircraft_id ?? '')
    })
})
const events = computed(() => {
  const id = workspace.workspace.selected_aircraft_id
  return id ? server.timelineEvents.filter((event) => event.aircraft_id === id) : []
})
const sample = computed(() => {
  const id = workspace.workspace.selected_aircraft_id
  return id ? server.samples[id] : null
})

function eventLabel(eventType: string): string {
  return t(`events.${eventType}`)
}

function aircraftLabel(id: string | undefined): string {
  if (!id) return '—'
  return server.aircraft.find((aircraft) => aircraft.id === id)?.name || id
}
</script>

<template>
  <aside class="editor-panel right-sidebar scrollbar-thin">
    <section class="editor-section">
      <h2 class="editor-section-header">{{ t('inspector.currentState') }}</h2>
      <div v-if="sample" class="editor-section-body text-xs">
        <div class="mono text-(--text-muted)">
          {{
            formatRelativeTime(
              sample.timestamp_secs - (server.playback?.bounds?.[0] ?? sample.timestamp_secs),
            )
          }}
          · {{ formatAbsoluteTime(sample.timestamp_secs, workspace.workspace.locale) }}
        </div>
        <pre class="state-json scrollbar-thin">{{ JSON.stringify(sample.state, null, 2) }}</pre>
      </div>
      <p v-else class="empty-copy">{{ t('inspector.noSample') }}</p>
    </section>

    <section v-if="currentFrameEvents.length" class="editor-section current-frame-events">
      <h2 class="editor-section-header">
        {{ t('inspector.currentFrameEvents', { count: currentFrameEvents.length }) }}
      </h2>
      <div class="editor-section-body">
        <div
          v-for="(event, index) in currentFrameEvents"
          :key="`${event.aircraft_id}:${event.timestamp_secs}:${event.event_type}:${event.name ?? event.reason ?? ''}:${index}`"
          class="current-frame-event-row"
        >
          <span
            class="current-frame-event-marker"
            :class="`current-frame-event-marker-${event.event_type}`"
          />
          <strong>{{ eventLabel(event.event_type) }}</strong>
          <span class="min-w-0 truncate text-(--text-secondary)">
            {{ event.name ?? event.reason ?? '' }}
          </span>
          <span class="current-frame-event-aircraft mono">
            {{ aircraftLabel(event.aircraft_id) }}
          </span>
        </div>
      </div>
    </section>

    <section class="editor-section">
      <h2 class="editor-section-header">{{ t('inspector.events') }}</h2>
      <div v-if="events.length" class="editor-section-body">
        <div
          v-for="event in events"
          :key="`${event.timestamp_secs}:${event.event_type}`"
          class="event-row"
        >
          <span class="mono text-(--text-muted)">{{
            formatRelativeTime(
              event.timestamp_secs - (server.playback?.bounds?.[0] ?? event.timestamp_secs),
            )
          }}</span>
          <strong>{{ eventLabel(event.event_type) }}</strong>
          <span class="min-w-0 truncate text-(--text-secondary)">{{
            event.name ?? event.reason ?? ''
          }}</span>
        </div>
      </div>
      <p v-else class="empty-copy">{{ t('inspector.noEvents') }}</p>
    </section>

    <section v-if="chart" class="editor-section">
      <h2 class="editor-section-header">{{ t('inspector.chart') }}</h2>
      <div class="editor-section-body">
        <label class="field-label">
          {{ t('inspector.title') }}
          <input v-model="chart.title" class="editor-input w-full" />
        </label>
        <label class="mt-2 flex items-center gap-2 text-xs">
          <input v-model="chart.legend_visible" type="checkbox" />{{ t('inspector.showLegend') }}
        </label>

        <article
          v-for="(curve, index) in chart.curves"
          :key="`${curve.aircraft_id}:${index}`"
          class="curve-editor"
        >
          <div class="flex items-center gap-2">
            <input
              v-model="curve.color"
              type="color"
              class="h-7 w-8 cursor-pointer border-0 bg-transparent"
            />
            <input v-model="curve.alias" class="editor-input min-w-0 flex-1" />
            <button
              class="editor-icon-button"
              :title="curve.visible ? t('inspector.hideCurve') : t('inspector.showCurve')"
              @click="curve.visible = !curve.visible"
            >
              <Eye v-if="curve.visible" class="h-3.5 w-3.5" />
              <EyeOff v-else class="h-3.5 w-3.5" />
            </button>
            <button
              class="editor-icon-button"
              :title="t('inspector.removeCurve')"
              @click="chart.curves.splice(index, 1)"
            >
              <Trash2 class="h-3.5 w-3.5" />
            </button>
          </div>
          <div class="mt-3 grid grid-cols-2 gap-2">
            <label class="field-label col-span-2">
              {{ t('inspector.aircraft') }}
              <select v-model="curve.aircraft_id" class="editor-select w-full">
                <option
                  v-if="!server.aircraft.some((aircraft) => aircraft.id === curve.aircraft_id)"
                  :value="curve.aircraft_id"
                  disabled
                >
                  {{
                    t('inspector.unavailableAircraft', {
                      id: curve.aircraft_id,
                    })
                  }}
                </option>
                <option v-for="aircraft in server.aircraft" :key="aircraft.id" :value="aircraft.id">
                  {{ aircraft.name || aircraft.id }}
                </option>
              </select>
            </label>
            <label class="field-label"
              >{{ t('inspector.pattern') }}
              <select v-model="curve.line_pattern" class="editor-select w-full">
                <option value="solid">{{ t('inspector.solid') }}</option>
                <option value="dashed">{{ t('inspector.dashed') }}</option>
                <option value="dotted">{{ t('inspector.dotted') }}</option>
              </select>
            </label>
            <label class="field-label"
              >{{ t('inspector.yAxis') }}
              <select v-model="curve.y_axis" class="editor-select w-full">
                <option value="left">{{ t('inspector.left') }}</option>
                <option value="right">{{ t('inspector.right') }}</option>
              </select>
            </label>
            <label class="field-label"
              >{{ t('inspector.width') }}
              <input
                v-model.number="curve.line_width"
                type="number"
                min="0.5"
                max="8"
                step="0.5"
                class="editor-input w-full"
              />
            </label>
            <label class="field-label"
              >{{ t('inspector.opacity') }}
              <input
                v-model.number="curve.opacity"
                type="number"
                min="0"
                max="1"
                step="0.1"
                class="editor-input w-full"
              />
            </label>
            <label class="field-label"
              >{{ t('inspector.scale') }}
              <input
                v-model.number="curve.scale"
                type="number"
                step="any"
                class="editor-input w-full"
              />
            </label>
            <label class="field-label"
              >{{ t('inspector.offset') }}
              <input
                v-model.number="curve.offset"
                type="number"
                step="any"
                class="editor-input w-full"
              />
            </label>
            <label class="field-label"
              >{{ t('inspector.unit') }}
              <input v-model="curve.unit" class="editor-input w-full" />
            </label>
            <label class="field-label"
              >{{ t('inspector.format') }}
              <select v-model="curve.value_format" class="editor-select w-full">
                <option value="auto">{{ t('inspector.auto') }}</option>
                <option value="fixed">{{ t('inspector.fixed') }}</option>
                <option value="scientific">{{ t('inspector.scientific') }}</option>
              </select>
            </label>
            <label class="field-label"
              >{{ t('inspector.precision') }}
              <input
                v-model.number="curve.precision"
                type="number"
                min="0"
                max="8"
                class="editor-input w-full"
              />
            </label>
            <label class="mt-5 flex items-center gap-2 text-xs"
              ><input v-model="curve.smooth" type="checkbox" />{{ t('inspector.smooth') }}</label
            >
            <label class="flex items-center gap-2 text-xs"
              ><input v-model="curve.show_symbol" type="checkbox" />{{
                t('inspector.symbols')
              }}</label
            >
          </div>
        </article>
      </div>
    </section>
  </aside>
</template>
