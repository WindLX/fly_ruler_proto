import { onBeforeUnmount, onMounted } from 'vue'

import { playbackShortcutFor, togglePlaybackAction } from '@/shortcuts'
import { useServerStore } from '@/stores/server'
import type { PlaybackStepDirection, PlaybackStepUnit } from '@/types'

interface QueuedStep {
  unit: PlaybackStepUnit
  direction: PlaybackStepDirection
  count: number
}

export function usePlaybackShortcuts() {
  const server = useServerStore()
  const queue: QueuedStep[] = []
  let draining = false
  let lastRepeatedStepAt = 0

  function enqueueStep(step: QueuedStep) {
    const previous = queue[queue.length - 1]
    if (
      previous &&
      previous.unit === step.unit &&
      previous.direction === step.direction &&
      previous.count + step.count <= 100
    ) {
      previous.count += step.count
    } else {
      queue.push(step)
    }
    void drainSteps()
  }

  async function drainSteps() {
    if (draining) return
    draining = true
    try {
      while (queue.length) {
        const step = queue.shift()
        if (!step) continue
        await server.step(step.unit, step.direction, step.count)
      }
    } catch (cause) {
      queue.length = 0
      server.reportError(cause)
    } finally {
      draining = false
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (document.querySelector('[aria-modal="true"]')) return
    const shortcut = playbackShortcutFor(event)
    if (!shortcut) return
    if (shortcut.kind === 'step' && event.repeat) {
      const now = Date.now()
      if (now - lastRepeatedStepAt < 50) return
      lastRepeatedStepAt = now
    }
    event.preventDefault()

    if (shortcut.kind === 'step') {
      enqueueStep(shortcut)
      return
    }
    if (shortcut.kind === 'bound') {
      const bounds = server.playback?.bounds
      if (bounds) void server.seek(shortcut.edge === 'start' ? bounds[0] : bounds[1])
      return
    }
    const action = togglePlaybackAction(server.playback?.mode)
    if (action === 'pause') {
      void server.pause().catch(server.reportError)
    } else {
      void server.play(server.playback?.speed ?? 1).catch(server.reportError)
    }
  }

  onMounted(() => window.addEventListener('keydown', handleKeydown))
  onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))
}
