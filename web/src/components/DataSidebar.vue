<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Database, Plus, Save, Trash2, Upload } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

import { api } from '@/api'
import { useSeriesStore } from '@/stores/series'
import { useServerStore } from '@/stores/server'
import { useWorkspaceStore } from '@/stores/workspace'
import { createCurveStyle, selectorKey } from '@/utils'

const server = useServerStore()
const workspace = useWorkspaceStore()
const series = useSeriesStore()
const { t } = useI18n()
const sessionName = ref('flight')
const selectedAircraft = computed(() => workspace.workspace.selected_aircraft_id)
const catalog = computed(() =>
  selectedAircraft.value ? (series.catalogs[selectedAircraft.value] ?? []) : [],
)
const grouped = computed(() =>
  catalog.value.reduce<Record<string, typeof catalog.value>>((groups, field) => {
    ;(groups[field.group] ??= []).push(field)
    return groups
  }, {}),
)

watch(
  selectedAircraft,
  (id) => {
    if (id) void series.loadCatalog(id)
  },
  { immediate: true },
)

async function saveSession() {
  await api.saveSession(sessionName.value, true)
}

async function loadSession(name: string) {
  await api.loadSession(name)
}

async function clearMemory() {
  if (window.confirm('Clear all in-memory flight data?')) await api.clear()
}
</script>

<template>
  <aside
    class="panel-surface flex min-h-0 w-78 shrink-0 scrollbar-thin flex-col overflow-y-auto rounded-lg"
  >
    <section class="border-b border-(--border-color) p-3">
      <h2 class="tool-label">{{ t('app.aircraft') }}</h2>
      <div class="mt-2 space-y-2">
        <button
          v-for="item in server.aircraft"
          :key="item.id"
          class="w-full rounded-md border px-3 py-2 text-left"
          :class="
            item.id === selectedAircraft
              ? 'border-(--accent) bg-(--accent-soft)'
              : 'border-(--border-color) bg-(--card-bg)'
          "
          @click="workspace.selectAircraft(item.id)"
        >
          <div class="truncate text-sm font-semibold">{{ item.name || item.id }}</div>
          <div class="mono mt-1 truncate text-[11px] text-(--text-muted)">{{ item.id }}</div>
          <div class="mt-1 text-xs text-(--text-secondary)">
            {{ item.state_count }} states · {{ item.event_count }} events
          </div>
        </button>
      </div>
    </section>

    <section class="border-b border-(--border-color) p-3">
      <h2 class="tool-label">{{ t('app.fields') }}</h2>
      <div v-if="selectedAircraft" class="mt-2 space-y-2">
        <details
          v-for="(fields, group) in grouped"
          :key="group"
          open
          class="rounded-md border border-(--border-color) bg-(--card-bg)"
        >
          <summary class="cursor-pointer px-3 py-2 text-xs font-semibold uppercase">
            {{ group }}
          </summary>
          <button
            v-for="(field, index) in fields"
            :key="selectorKey(field.selector)"
            class="flex w-full items-center gap-2 border-t border-(--border-color) px-3 py-2 text-left text-xs hover:bg-(--panel-muted)"
            @click="workspace.addToBasket(createCurveStyle(selectedAircraft, field, index))"
          >
            <Plus class="h-3.5 w-3.5 text-(--accent)" />
            <span class="min-w-0 flex-1 truncate">{{ field.label }}</span>
            <span class="text-(--text-muted)">{{ field.unit }}</span>
          </button>
        </details>
      </div>
      <p v-else class="mt-3 text-xs text-(--text-muted)">{{ t('app.noData') }}</p>
    </section>

    <section class="p-3">
      <h2 class="tool-label">{{ t('app.sessions') }}</h2>
      <div class="mt-2 flex gap-2">
        <input v-model="sessionName" class="toolbar-input min-w-0 flex-1" />
        <button class="icon-button" title="Save" @click="saveSession">
          <Save class="h-4 w-4" />
        </button>
      </div>
      <button
        v-for="item in server.sessions"
        :key="item.name"
        class="mt-2 flex w-full items-center gap-2 rounded-md border border-(--border-color) bg-(--card-bg) px-3 py-2 text-left text-sm"
        @click="loadSession(item.name)"
      >
        <Database class="h-4 w-4" /><span class="flex-1 truncate">{{ item.name }}</span>
        <Upload class="h-3.5 w-3.5" />
      </button>
      <button class="danger-button mt-3 w-full" @click="clearMemory">
        <Trash2 class="h-4 w-4" />{{ t('app.clear') }}
      </button>
    </section>
  </aside>
</template>
