<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'

const server = useServerStore()
const workspace = useWorkspaceStore()
const preview = ref(0)
const bounds = computed<[number, number]>(() => server.playback?.bounds ?? [0, 1])

watch(
  () => server.playback?.cursor_secs,
  (cursor) => {
    if (cursor !== null && cursor !== undefined) preview.value = cursor
  },
  { immediate: true },
)

async function commitSeek() {
  await server.seek(preview.value)
}

function setQueryRange() {
  workspace.workspace.query_start = bounds.value[0]
  workspace.workspace.query_end = bounds.value[1]
}
</script>

<template>
  <footer
    class="flex min-h-12 items-center gap-3 border-t border-(--border-color) bg-(--panel-bg) px-3"
  >
    <span class="mono w-30 text-xs text-(--text-muted)">{{ bounds[0].toFixed(3) }}</span>
    <input
      v-model.number="preview"
      type="range"
      class="min-w-0 flex-1"
      :min="bounds[0]"
      :max="bounds[1]"
      :step="Math.max((bounds[1] - bounds[0]) / 100000, 0.001)"
      @change="commitSeek"
    />
    <span class="mono w-30 text-right text-xs text-(--text-muted)">{{ bounds[1].toFixed(3) }}</span>
    <button class="toolbar-button" @click="setQueryRange">Full range</button>
  </footer>
</template>
