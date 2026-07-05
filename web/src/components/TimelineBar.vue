<script setup lang="ts">
import { ChevronDown, ChevronLeft, ChevronRight, ChevronUp, Maximize2 } from 'lucide-vue-next'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import {
  formatAbsoluteTime,
  formatRelativeTime,
  generateTimelineTicks,
  toRelativeTime,
} from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const { t } = useI18n()
const track = ref<HTMLElement | null>(null)
const trackWidth = ref(1)
const preview = ref(0)
const dragging = ref(false)
let resizeObserver: ResizeObserver | null = null

const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])
const span = computed(() => Math.max(bounds.value[1] - bounds.value[0], 0.001))
const ticks = computed(() => generateTimelineTicks(span.value, trackWidth.value))

const eventClusters = computed(() => {
  const clusters: Array<{
    pixel: number
    percent: number
    events: typeof server.timelineEvents
  }> = []
  for (const event of server.timelineEvents) {
    const percent = ((event.timestamp_secs - bounds.value[0]) / span.value) * 100
    const pixel = (percent / 100) * trackWidth.value
    const previous = clusters[clusters.length - 1]
    if (previous && Math.abs(previous.pixel - pixel) < 10) {
      previous.events.push(event)
      previous.pixel =
        previous.events.reduce(
          (sum, item) =>
            sum + ((item.timestamp_secs - bounds.value[0]) / span.value) * trackWidth.value,
          0,
        ) / previous.events.length
      previous.percent = (previous.pixel / trackWidth.value) * 100
    } else {
      clusters.push({ pixel, percent, events: [event] })
    }
  }
  return clusters
})

const cursorPercent = computed(() =>
  Math.min(100, Math.max(0, ((preview.value - bounds.value[0]) / span.value) * 100)),
)

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

onMounted(() => {
  resizeObserver = new ResizeObserver(([entry]) => {
    if (entry) trackWidth.value = Math.max(1, entry.contentRect.width)
  })
  if (track.value) resizeObserver.observe(track.value)
})

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

function stepEvent(direction: 'previous' | 'next') {
  void server.step('event', direction, 1)
}

function stepSample(direction: 'previous' | 'next') {
  void server.step('sample', direction, 1)
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
  resizeObserver?.disconnect()
  window.removeEventListener('pointermove', moveDrag)
  window.removeEventListener('pointerup', finishDrag)
})
</script>

<template>
  <footer class="timeline-editor">
    <div class="timeline-header">
      <div class="transport-group">
        <button
          class="editor-icon-button"
          :title="`${t('timeline.previousFrame')} (←)`"
          @click="stepSample('previous')"
        >
          <ChevronLeft class="h-3.5 w-3.5" />
        </button>
        <button
          class="editor-icon-button"
          :title="`${t('timeline.nextFrame')} (→)`"
          @click="stepSample('next')"
        >
          <ChevronRight class="h-3.5 w-3.5" />
        </button>
        <span class="timeline-divider" />
        <button
          class="editor-icon-button"
          :title="`${t('timeline.previousEvent')} (↑)`"
          @click="stepEvent('previous')"
        >
          <ChevronUp class="h-3.5 w-3.5" />
        </button>
        <button
          class="editor-icon-button"
          :title="`${t('timeline.nextEvent')} (↓)`"
          @click="stepEvent('next')"
        >
          <ChevronDown class="h-3.5 w-3.5" />
        </button>
      </div>

      <span class="timeline-time mono">{{ formatRelativeTime(preview - bounds[0]) }}</span>
      <span class="timeline-absolute">
        {{ formatAbsoluteTime(preview, workspace.workspace.locale) }}
      </span>
      <span v-if="server.timelineTruncated" class="timeline-warning">
        {{ t('timeline.truncated') }}
      </span>
      <div class="timeline-shortcuts">
        <span><kbd>Space</kbd> {{ t('shortcuts.playPause') }}</span>
        <span><kbd>← →</kbd> {{ t('shortcuts.frame') }}</span>
        <span><kbd>↑ ↓</kbd> {{ t('shortcuts.event') }}</span>
      </div>
      <button class="editor-button ml-auto" @click="setQueryRange">
        <Maximize2 class="h-3.5 w-3.5" />{{ t('timeline.fullRange') }}
      </button>
    </div>

    <div ref="track" class="timeline-track" data-testid="timeline-track" @pointerdown="startDrag">
      <div
        v-if="queryRange"
        class="timeline-query-range"
        :style="{ left: `${queryRange.left}%`, width: `${queryRange.width}%` }"
      />
      <div class="timeline-axis" />
      <div
        v-for="tick in ticks"
        :key="`${tick.level}:${tick.value}`"
        class="timeline-tick"
        :class="`timeline-tick-${tick.level}`"
        :style="{ left: `${(tick.value / span) * 100}%` }"
      >
        <span v-if="tick.label" class="timeline-tick-label mono">{{ tick.label }}</span>
      </div>
      <button
        v-for="cluster in eventClusters"
        :key="`${cluster.pixel}:${cluster.events.length}`"
        class="timeline-event"
        :class="`timeline-event-${cluster.events[0]?.event_type}`"
        :style="{ left: `${cluster.percent}%` }"
        :title="eventTitle(cluster)"
        @pointerdown.stop
        @click.stop="jumpCluster(cluster)"
      >
        <span v-if="cluster.events.length > 1">{{ cluster.events.length }}</span>
      </button>
      <div class="timeline-playhead" :style="{ left: `${cursorPercent}%` }">
        <div class="timeline-playhead-handle" />
      </div>
    </div>

    <div class="timeline-status">
      <span>{{ formatAbsoluteTime(bounds[0], workspace.workspace.locale) }}</span>
      <span class="mono">{{ t('timeline.duration') }} {{ formatRelativeTime(span) }}</span>
      <span>{{ formatAbsoluteTime(bounds[1], workspace.workspace.locale) }}</span>
    </div>
  </footer>
</template>
