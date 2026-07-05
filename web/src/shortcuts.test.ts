import { describe, expect, it } from 'vitest'

import { playbackShortcutFor, togglePlaybackAction } from '@/shortcuts'

function keyEvent(key: string, init: KeyboardEventInit & { target?: EventTarget | null } = {}) {
  return {
    key,
    code: key === ' ' ? 'Space' : '',
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    repeat: false,
    target: null,
    ...init,
  } as KeyboardEvent
}

describe('playback shortcuts', () => {
  it('maps Blender-style playback controls', () => {
    expect(playbackShortcutFor(keyEvent('ArrowLeft'))).toEqual({
      kind: 'step',
      unit: 'sample',
      direction: 'previous',
      count: 1,
    })
    expect(playbackShortcutFor(keyEvent('ArrowRight', { shiftKey: true }))).toEqual({
      kind: 'step',
      unit: 'sample',
      direction: 'next',
      count: 10,
    })
    expect(playbackShortcutFor(keyEvent('ArrowUp'))).toMatchObject({
      unit: 'event',
      direction: 'previous',
    })
    expect(playbackShortcutFor(keyEvent('Home'))).toEqual({ kind: 'bound', edge: 'start' })
  })

  it('ignores repeated space and editable controls', () => {
    expect(playbackShortcutFor(keyEvent(' ', { repeat: true }))).toBeNull()
    expect(playbackShortcutFor(keyEvent('ArrowRight'))).not.toBeNull()
    expect(
      playbackShortcutFor(
        keyEvent('ArrowRight', { target: { tagName: 'INPUT' } as unknown as EventTarget }),
      ),
    ).toBeNull()
  })

  it('toggles live and playing modes to pause', () => {
    expect(togglePlaybackAction('live')).toBe('pause')
    expect(togglePlaybackAction('replay_playing')).toBe('pause')
    expect(togglePlaybackAction('replay_paused')).toBe('play')
  })
})
