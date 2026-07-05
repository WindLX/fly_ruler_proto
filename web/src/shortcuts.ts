import type { PlaybackMode, PlaybackStepDirection, PlaybackStepUnit } from '@/types'

export type PlaybackShortcut =
  | { kind: 'toggle' }
  | { kind: 'step'; unit: PlaybackStepUnit; direction: PlaybackStepDirection; count: number }
  | { kind: 'bound'; edge: 'start' | 'end' }

export function playbackShortcutFor(event: KeyboardEvent): PlaybackShortcut | null {
  if (event.altKey || event.ctrlKey || event.metaKey) return null
  if (isEditableTarget(event.target)) return null
  if (event.code === 'Space') return event.repeat ? null : { kind: 'toggle' }
  if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
    return {
      kind: 'step',
      unit: 'sample',
      direction: event.key === 'ArrowLeft' ? 'previous' : 'next',
      count: event.shiftKey ? 10 : 1,
    }
  }
  if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
    return {
      kind: 'step',
      unit: 'event',
      direction: event.key === 'ArrowUp' ? 'previous' : 'next',
      count: 1,
    }
  }
  if (event.key === 'Home') return { kind: 'bound', edge: 'start' }
  if (event.key === 'End') return { kind: 'bound', edge: 'end' }
  return null
}

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || typeof target !== 'object') return false
  const element = target as { isContentEditable?: boolean; tagName?: string }
  if (element.isContentEditable) return true
  return (
    typeof element.tagName === 'string' && ['INPUT', 'TEXTAREA', 'SELECT'].includes(element.tagName)
  )
}

export function togglePlaybackAction(mode: PlaybackMode | undefined): 'pause' | 'play' {
  return mode === 'replay_playing' || mode === 'live' ? 'pause' : 'play'
}
