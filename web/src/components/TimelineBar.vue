<script setup lang="ts">
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  RotateCcw,
  ZoomIn,
  ZoomOut,
} from 'lucide-vue-next'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import {
  formatAbsoluteTime,
  formatRelativeTime,
  generateTimelineTicks,
  toRelativeTime,
  zoomTimeRange,
} from '@/utils'

const TRACK_SIDE_PADDING = 16
const EVENT_CLUSTER_DISTANCE = 10

const server = useServerStore()
const workspace = useWorkspaceStore()
const { t } = useI18n()
const track = ref<HTMLElement | null>(null)
const trackWidth = ref(1)
const preview = ref(0)
const dragging = ref(false)
const viewStart = ref(0)
const viewEnd = ref(1)
let resizeObserver: ResizeObserver | null = null

const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])
const fullSpan = computed(() => Math.max(bounds.value[1] - bounds.value[0], 0.001))
const visibleRange = computed<[number, number]>(() => [viewStart.value, viewEnd.value])
const visibleSpan = computed(() => Math.max(viewEnd.value - viewStart.value, 0.001))
const minimumSpan = computed(() => Math.max(fullSpan.value / 10_000, 0.001))
const contentWidth = computed(() => Math.max(1, trackWidth.value - TRACK_SIDE_PADDING * 2))
const viewOffset = computed(() => viewStart.value - bounds.value[0])
const ticks = computed(() =>
  generateTimelineTicks(visibleSpan.value, contentWidth.value, 84, viewOffset.value),
)
const zoomLevel = computed(() => fullSpan.value / visibleSpan.value)
const zoomLevelLabel = computed(() =>
  zoomLevel.value < 10 ? `${zoomLevel.value.toFixed(1)}×` : `${Math.round(zoomLevel.value)}×`,
)
const isFullRange = computed(
  () =>
    Math.abs(viewStart.value - bounds.value[0]) <= 1e-9 &&
    Math.abs(viewEnd.value - bounds.value[1]) <= 1e-9,
)
const canZoomIn = computed(() => visibleSpan.value > minimumSpan.value * 1.001)

const eventClusters = computed(() => {
  const clusters: Array<{ pixel: number; events: typeof server.timelineEvents }> = []
  for (const event of server.timelineEvents) {
    if (
      event.timestamp_secs < viewStart.value - 1e-9 ||
      event.timestamp_secs > viewEnd.value + 1e-9
    ) {
      continue
    }
    const pixel = timeToPixel(event.timestamp_secs)
    const previous = clusters[clusters.length - 1]
    if (previous && Math.abs(previous.pixel - pixel) < EVENT_CLUSTER_DISTANCE) {
      previous.events.push(event)
      previous.pixel =
        previous.events.reduce((sum, item) => sum + timeToPixel(item.timestamp_secs), 0) /
        previous.events.length
    } else {
      clusters.push({ pixel, events: [event] })
    }
  }
  return clusters
})

const cursorVisible = computed(
  () => preview.value >= viewStart.value - 1e-9 && preview.value <= viewEnd.value + 1e-9,
)
const cursorPixel = computed(() => timeToPixel(preview.value))

const queryRange = computed(() => {
  const start = workspace.workspace.query_start
  const end = workspace.workspace.query_end
  if (start === null || end === null) return null
  const clippedStart = Math.max(start, viewStart.value)
  const clippedEnd = Math.min(end, viewEnd.value)
  if (clippedEnd < clippedStart) return null
  return {
    left: timeToPixel(clippedStart),
    width: Math.max(1, timeToPixel(clippedEnd) - timeToPixel(clippedStart)),
  }
})

watch(
  [() => server.playback?.bounds?.[0], () => server.playback?.bounds?.[1]],
  ([start, end], [oldStart, oldEnd]) => {
    if (typeof start !== 'number' || typeof end !== 'number' || end <= start) {
      viewStart.value = 0
      viewEnd.value = 1
      return
    }
    const followedFullRange =
      typeof oldStart !== 'number' ||
      typeof oldEnd !== 'number' ||
      (Math.abs(viewStart.value - oldStart) <= 1e-9 && Math.abs(viewEnd.value - oldEnd) <= 1e-9)
    if (followedFullRange || viewEnd.value <= start || viewStart.value >= end) {
      viewStart.value = start
      viewEnd.value = end
      return
    }
    const retainedSpan = Math.min(viewEnd.value - viewStart.value, end - start)
    viewStart.value = Math.max(start, Math.min(viewStart.value, end - retainedSpan))
    viewEnd.value = viewStart.value + retainedSpan
  },
  { immediate: true },
)

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

function timeToPixel(timestamp: number): number {
  const ratio = (timestamp - viewStart.value) / visibleSpan.value
  return TRACK_SIDE_PADDING + Math.min(1, Math.max(0, ratio)) * contentWidth.value
}

function timestampFromClientX(clientX: number): number {
  const rect = track.value?.getBoundingClientRect()
  if (!rect) return preview.value
  const ratio = Math.min(
    1,
    Math.max(0, (clientX - rect.left - TRACK_SIDE_PADDING) / contentWidth.value),
  )
  return viewStart.value + ratio * visibleSpan.value
}

function timestampFromPointer(event: PointerEvent): number {
  return timestampFromClientX(event.clientX)
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

function applyZoom(factor: number, anchor = zoomAnchor()) {
  const [start, end] = zoomTimeRange(
    bounds.value,
    visibleRange.value,
    anchor,
    factor,
    minimumSpan.value,
  )
  viewStart.value = start
  viewEnd.value = end
}

function zoomAnchor(): number {
  return preview.value >= viewStart.value && preview.value <= viewEnd.value
    ? preview.value
    : (viewStart.value + viewEnd.value) / 2
}

function handleWheel(event: WheelEvent) {
  const factor = Math.min(2, Math.max(0.5, Math.exp(event.deltaY * 0.0015)))
  applyZoom(factor, timestampFromClientX(event.clientX))
}

function resetZoom() {
  viewStart.value = bounds.value[0]
  viewEnd.value = bounds.value[1]
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
        <span><kbd>Shift + ← →</kbd> {{ t('shortcuts.tenFrame') }}</span>
        <span><kbd>↑ ↓</kbd> {{ t('shortcuts.event') }}</span>
        <span>{{ t('shortcuts.wheelZoom') }}</span>
      </div>
      <div class="timeline-zoom-controls">
        <span class="timeline-zoom-level mono">{{ zoomLevelLabel }}</span>
        <button
          class="editor-icon-button"
          :disabled="!canZoomIn"
          :title="t('timeline.zoomIn')"
          @click="applyZoom(0.5)"
        >
          <ZoomIn class="h-3.5 w-3.5" />
        </button>
        <button
          class="editor-icon-button"
          :disabled="isFullRange"
          :title="t('timeline.zoomOut')"
          @click="applyZoom(2)"
        >
          <ZoomOut class="h-3.5 w-3.5" />
        </button>
        <button
          class="editor-icon-button"
          :disabled="isFullRange"
          :title="t('timeline.resetZoom')"
          @click="resetZoom"
        >
          <RotateCcw class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <div
      ref="track"
      class="timeline-track"
      data-testid="timeline-track"
      @pointerdown="startDrag"
      @wheel.prevent="handleWheel"
    >
      <div
        v-if="queryRange"
        class="timeline-query-range"
        :style="{ left: `${queryRange.left}px`, width: `${queryRange.width}px` }"
      />
      <div class="timeline-axis" />
      <div
        v-for="tick in ticks"
        :key="`${tick.level}:${tick.value}`"
        class="timeline-tick"
        :class="`timeline-tick-${tick.level}`"
        :style="{
          left: `${TRACK_SIDE_PADDING + ((tick.value - viewOffset) / visibleSpan) * contentWidth}px`,
        }"
      >
        <span v-if="tick.label" class="timeline-tick-label mono">{{ tick.label }}</span>
      </div>
      <button
        v-for="cluster in eventClusters"
        :key="`${cluster.pixel}:${cluster.events.length}`"
        class="timeline-event"
        :class="`timeline-event-${cluster.events[0]?.event_type}`"
        :style="{ left: `${cluster.pixel}px` }"
        :title="eventTitle(cluster)"
        @pointerdown.stop
        @click.stop="jumpCluster(cluster)"
      >
        <span v-if="cluster.events.length > 1">{{ cluster.events.length }}</span>
      </button>
      <div v-if="cursorVisible" class="timeline-playhead" :style="{ left: `${cursorPixel}px` }">
        <div class="timeline-playhead-handle" />
      </div>
    </div>

    <div class="timeline-status">
      <span>{{ formatAbsoluteTime(viewStart, workspace.workspace.locale) }}</span>
      <span class="mono">{{ t('timeline.duration') }} {{ formatRelativeTime(visibleSpan) }}</span>
      <span>{{ formatAbsoluteTime(viewEnd, workspace.workspace.locale) }}</span>
    </div>
  </footer>
</template>
