import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { api, websocketUrl } from '@/api'
import type {
  AircraftEvent,
  AircraftSummary,
  OperationRecord,
  ServerStatus,
  SessionSummary,
  SnapshotMessage,
  TimestampedState,
} from '@/types'

export const useServerStore = defineStore('server', () => {
  const connected = ref(false)
  const status = ref<ServerStatus | null>(null)
  const aircraft = ref<AircraftSummary[]>([])
  const sessions = ref<SessionSummary[]>([])
  const samples = ref<Record<string, TimestampedState>>({})
  const operations = ref<Record<string, OperationRecord>>({})
  const timelineEvents = ref<AircraftEvent[]>([])
  const timelineTruncated = ref(false)
  const error = ref<string | null>(null)
  const storeRevision = ref(0)
  const workspaceRevision = ref(0)
  let socket: WebSocket | null = null
  let reconnectTimer: number | null = null

  const playback = computed(() => status.value?.playback ?? null)

  async function refreshTimeline(bounds = status.value?.playback.bounds ?? null) {
    if (bounds) {
      const events = await api.timelineEvents(bounds[0], bounds[1])
      timelineEvents.value = events.items
      timelineTruncated.value = events.total > events.items.length
    } else {
      timelineEvents.value = []
      timelineTruncated.value = false
    }
  }

  async function refresh() {
    try {
      const [nextStatus, nextAircraft, nextSessions] = await Promise.all([
        api.status(),
        api.aircraft(),
        api.sessions(),
      ])
      status.value = nextStatus
      aircraft.value = nextAircraft.aircraft
      sessions.value = nextSessions.sessions
      await refreshTimeline(nextStatus.playback.bounds)
      error.value = null
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    }
  }

  function connect() {
    socket?.close()
    socket = new WebSocket(websocketUrl())
    socket.onopen = () => {
      connected.value = true
      error.value = null
    }
    socket.onmessage = (event) => {
      const message = JSON.parse(String(event.data)) as
        | SnapshotMessage
        | { type: 'operation_status'; operation: OperationRecord }
        | { type: 'store_changed'; reason: string }
        | { type: 'workspace_changed'; revision: number }
      if (message.type === 'snapshot') {
        const previousEventCount = status.value?.store.event_count
        status.value = status.value
          ? { ...status.value, playback: message.playback, store: message.store }
          : null
        const next = { ...samples.value }
        for (const [id, aircraftSnapshot] of Object.entries(message.aircraft)) {
          next[id] = aircraftSnapshot.sample
        }
        samples.value = next
        if (previousEventCount !== message.store.event_count) {
          void refreshTimeline(message.playback.bounds)
        }
      } else if (message.type === 'operation_status') {
        operations.value = { ...operations.value, [message.operation.id]: message.operation }
        if (message.operation.state === 'succeeded' || message.operation.state === 'failed') {
          void refresh()
        }
      } else if (message.type === 'store_changed') {
        storeRevision.value++
        void refresh()
      } else if (message.type === 'workspace_changed') {
        workspaceRevision.value = message.revision
      }
    }
    socket.onclose = () => {
      connected.value = false
      reconnectTimer = window.setTimeout(connect, 1500)
    }
    socket.onerror = () => {
      error.value = 'connection.websocketFailed'
    }
  }

  function stop() {
    if (reconnectTimer !== null) window.clearTimeout(reconnectTimer)
    socket?.close()
    socket = null
  }

  async function setLive() {
    const next = await api.live()
    if (status.value) status.value.playback = next
  }

  async function pause() {
    const next = await api.pause()
    if (status.value) status.value.playback = next
  }

  async function play(speed: number) {
    const next = await api.play(speed)
    if (status.value) status.value.playback = next
  }

  async function seek(timestamp: number) {
    const next = await api.seek(timestamp)
    if (status.value) status.value.playback = next
  }

  return {
    connected,
    status,
    aircraft,
    sessions,
    samples,
    operations,
    timelineEvents,
    timelineTruncated,
    error,
    playback,
    storeRevision,
    workspaceRevision,
    refresh,
    refreshTimeline,
    connect,
    stop,
    setLive,
    pause,
    play,
    seek,
  }
})
