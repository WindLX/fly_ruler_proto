<script setup lang="ts">
import { ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import { formatAbsoluteTime, formatRelativeTime, niceTickStep, toRelativeTime } from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const { t } = useI18n()
const track = ref<HTMLElement | null>(null)
const preview = ref(0)
const dragging = ref(false)
const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])
const span = computed(() => Math.max(bounds.value[1] - bounds.value[0], 0.001))

const ticks = computed(() => {
  const step = niceTickStep(span.value, 9)
  const values: number[] = []
  for (let value = step; value < span.value - step * 0.25; value += step) values.push(value)
  return values
})

const eventClusters = computed(() => {
  const clusters: Array<{ percent: number; events: typeof server.timelineEvents }> = []
  for (const event of server.timelineEvents) {
    const percent = ((event.timestamp_secs - bounds.value[0]) / span.value) * 100
    const previous = clusters[clusters.length - 1]
    if (previous && Math.abs(previous.percent - percent) < 0.75) {
      previous.events.push(event)
      previous.percent =
        previous.events.reduce(
          (sum, item) => sum + ((item.timestamp_secs - bounds.value[0]) / span.value) * 100,
          0,
        ) / previous.events.length
    } else {
      clusters.push({ percent, events: [event] })
    }
  }
  return clusters
})

const cursorPercent = computed(() => ((preview.value - bounds.value[0]) / span.value) * 100)

const queryRange = computed(() => {
  const start = workspace.workspace.query_start
  const end = workspace.workspace.query_end
  if (start === null || end === null) return null
  return {
    left: ((start - bounds.value[0]) / span.value) * 100,
    width: ((end - start) / span.value) * 100,
  }
})

watch(
  () => server.playback?.cursor_secs,
  (cursor) => {
    if (!dragging.value && cursor !== null && cursor !== undefined) preview.value = cursor
  },
  { immediate: true },
)

function timestampFromPointer(event: PointerEvent): number {
  const rect = track.value?.getBoundingClientRect()
  if (!rect) return preview.value
  const ratio = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width))
  return bounds.value[0] + ratio * span.value
}

function startDrag(event: PointerEvent) {
  dragging.value = true
  preview.value = timestampFromPointer(event)
  window.addEventListener('pointermove', moveDrag)
  window.addEventListener('pointerup', finishDrag, { once: true })
}

function moveDrag(event: PointerEvent) {
  if (dragging.value) preview.value = timestampFromPointer(event)
}

function finishDrag(event: PointerEvent) {
  preview.value = timestampFromPointer(event)
  dragging.value = false
  window.removeEventListener('pointermove', moveDrag)
  void server.seek(preview.value)
}

function jumpCluster(cluster: (typeof eventClusters.value)[number]) {
  const cursor = server.playback?.cursor_secs ?? preview.value
  const target = cluster.events.reduce((best, event) =>
    Math.abs(event.timestamp_secs - cursor) < Math.abs(best.timestamp_secs - cursor) ? event : best,
  )
  preview.value = target.timestamp_secs
  void server.seek(target.timestamp_secs)
}

function jumpEvent(direction: -1 | 1) {
  const cursor = server.playback?.cursor_secs ?? preview.value
  const events = server.timelineEvents
  const target =
    direction < 0
      ? [...events].reverse().find((event) => event.timestamp_secs < cursor - 1e-9)
      : events.find((event) => event.timestamp_secs > cursor + 1e-9)
  if (target) {
    preview.value = target.timestamp_secs
    void server.seek(target.timestamp_secs)
  }
}

function setQueryRange() {
  workspace.workspace.query_start = bounds.value[0]
  workspace.workspace.query_end = bounds.value[1]
}

function eventTitle(cluster: (typeof eventClusters.value)[number]): string {
  return cluster.events
    .slice(0, 8)
    .map(
      (event) =>
        `${formatRelativeTime(toRelativeTime(event.timestamp_secs, bounds.value[0]))} · ${t(`events.${event.event_type}`)} · ${event.aircraft_id ?? ''} ${event.name ?? event.reason ?? ''}`,
    )
    .join('\n')
}

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', moveDrag)
  window.removeEventListener('pointerup', finishDrag)
})
</script>

<template>
  <footer class="timeline border-t border-(--border-color) bg-(--panel-bg) px-3 py-2">
    <div class="mb-1 flex items-center gap-2 text-xs">
      <button
        class="icon-button h-7 w-7"
        :title="t('timeline.previousEvent')"
        @click="jumpEvent(-1)"
      >
        <ChevronLeft class="h-4 w-4" />
      </button>
      <button class="icon-button h-7 w-7" :title="t('timeline.nextEvent')" @click="jumpEvent(1)">
        <ChevronRight class="h-4 w-4" />
      </button>
      <span class="mono font-semibold">{{ formatRelativeTime(preview - bounds[0]) }}</span>
      <span class="text-(--text-muted)">{{
        formatAbsoluteTime(preview, workspace.workspace.locale)
      }}</span>
      <span v-if="server.timelineTruncated" class="text-amber-500">{{
        t('timeline.truncated')
      }}</span>
      <button class="toolbar-button ml-auto" @click="setQueryRange">
        {{ t('timeline.fullRange') }}
      </button>
    </div>

    <div
      ref="track"
      class="timeline-track relative h-11 cursor-ew-resize select-none"
      @pointerdown="startDrag"
    >
      <div
        v-if="queryRange"
        class="absolute top-4 h-3 rounded bg-(--accent-soft)"
        :style="{ left: `${queryRange.left}%`, width: `${queryRange.width}%` }"
      />
      <div class="absolute top-5 right-0 left-0 h-px bg-(--border-strong)" />
      <div
        v-for="tick in ticks"
        :key="tick"
        class="absolute top-3 h-5 w-px bg-(--border-color)"
        :style="{ left: `${(tick / span) * 100}%` }"
      >
        <span
          class="mono absolute top-5 -translate-x-1/2 text-[10px] whitespace-nowrap text-(--text-muted)"
        >
          {{ formatRelativeTime(tick, false) }}
        </span>
      </div>
      <button
        v-for="cluster in eventClusters"
        :key="`${cluster.percent}:${cluster.events.length}`"
        class="timeline-event absolute top-0 z-10 -translate-x-1/2"
        :class="`timeline-event-${cluster.events[0]?.event_type}`"
        :style="{ left: `${cluster.percent}%` }"
        :title="eventTitle(cluster)"
        @pointerdown.stop
        @click.stop="jumpCluster(cluster)"
      >
        <span v-if="cluster.events.length > 1">{{ cluster.events.length }}</span>
      </button>
      <div
        class="pointer-events-none absolute top-0 z-20 h-9 w-px bg-(--cursor-color)"
        :style="{ left: `${cursorPercent}%` }"
      >
        <div class="absolute top-0 -left-1 h-2 w-2 rotate-45 bg-(--cursor-color)" />
      </div>
    </div>

    <div class="mt-3 flex justify-between text-[10px] text-(--text-muted)">
      <span>{{ formatAbsoluteTime(bounds[0], workspace.workspace.locale) }}</span>
      <span>{{ formatAbsoluteTime(bounds[1], workspace.workspace.locale) }}</span>
    </div>
  </footer>
</template>
