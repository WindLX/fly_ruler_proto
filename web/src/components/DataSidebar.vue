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
const { t, te } = useI18n()
const sessionName = ref('flight')
const sessionBusy = ref(false)
const notice = ref<{ kind: 'success' | 'error'; text: string } | null>(null)
const confirmOpen = ref(false)
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
  await runSessionAction(async () => {
    await api.saveSession(sessionName.value, true)
    return t('sessions.saveQueued')
  })
}

async function loadSession(name: string) {
  await runSessionAction(async () => {
    await api.loadSession(name)
    return t('sessions.loadQueued', { name })
  })
}

async function clearMemory() {
  confirmOpen.value = true
}

async function confirmClearMemory() {
  confirmOpen.value = false
  await runSessionAction(async () => {
    await api.clear()
    await server.refresh()
    return t('sessions.cleared')
  })
}

function cancelClearMemory() {
  confirmOpen.value = false
}

async function runSessionAction(action: () => Promise<string>) {
  sessionBusy.value = true
  notice.value = null
  try {
    notice.value = { kind: 'success', text: await action() }
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause)
    notice.value = { kind: 'error', text: t('sessions.operationFailed', { message }) }
  } finally {
    sessionBusy.value = false
  }
}

function groupLabel(group: string): string {
  const key = `fields.groups.${group}`
  return te(key) ? t(key) : group
}

function fieldLabel(field: (typeof catalog.value)[number]): string {
  if (field.selector.kind === 'standard') {
    const key = `fields.paths.${field.selector.path.split('.').join('_')}`
    return te(key) ? t(key) : field.label
  }
  if (field.selector.kind === 'engine_throttle') {
    return t('fields.engineThrottle', { index: field.selector.index })
  }
  return field.label
}

function addField(field: (typeof catalog.value)[number], index: number) {
  if (!selectedAircraft.value) return
  workspace.addToBasket(
    createCurveStyle(selectedAircraft.value, { ...field, label: fieldLabel(field) }, index),
  )
}
</script>

<template>
  <aside class="editor-panel left-sidebar scrollbar-thin">
    <section class="editor-section">
      <h2 class="editor-section-header">{{ t('sidebar.aircraft') }}</h2>
      <div class="editor-section-body">
        <button
          v-for="item in server.aircraft"
          :key="item.id"
          class="data-card"
          :class="
            item.id === selectedAircraft
              ? 'border-(--accent) bg-(--accent-soft)'
              : 'border-(--border-color) bg-(--card-bg)'
          "
          @click="workspace.selectAircraft(item.id)"
        >
          <div class="truncate font-semibold">{{ item.name || item.id }}</div>
          <div class="mono truncate text-[10px] text-(--text-muted)">{{ item.id }}</div>
          <div class="text-[11px] text-(--text-secondary)">
            {{
              t('sidebar.statesAndEvents', {
                states: item.state_count,
                events: item.event_count,
              })
            }}
          </div>
        </button>
        <p v-if="server.aircraft.length === 0" class="empty-copy">
          {{ t('sidebar.noAircraft') }}
        </p>
      </div>
    </section>

    <section class="editor-section">
      <h2 class="editor-section-header">{{ t('sidebar.fields') }}</h2>
      <div v-if="selectedAircraft" class="editor-section-body catalog-body">
        <details v-for="(fields, group) in grouped" :key="group" open class="catalog-group">
          <summary class="catalog-summary">
            {{ groupLabel(String(group)) }}
          </summary>
          <button
            v-for="(field, index) in fields"
            :key="selectorKey(field.selector)"
            class="catalog-field"
            @click="addField(field, index)"
          >
            <Plus class="h-3.5 w-3.5 text-(--accent)" />
            <span class="min-w-0 flex-1 truncate">{{ fieldLabel(field) }}</span>
            <span class="text-(--text-muted)">{{ field.unit }}</span>
          </button>
        </details>
      </div>
      <p v-else class="empty-copy">{{ t('app.noData') }}</p>
    </section>

    <section class="editor-section">
      <h2 class="editor-section-header">{{ t('sessions.title') }}</h2>
      <div class="editor-section-body">
        <div class="flex gap-1">
          <input
            v-model="sessionName"
            class="editor-input min-w-0 flex-1"
            :placeholder="t('sessions.namePlaceholder')"
          />
          <button
            class="editor-icon-button"
            :disabled="sessionBusy"
            :title="t('sessions.saveTitle')"
            @click="saveSession"
          >
            <Save class="h-4 w-4" />
          </button>
        </div>
        <button
          v-for="item in server.sessions"
          :key="item.name"
          class="session-row"
          :disabled="sessionBusy"
          @click="loadSession(item.name)"
        >
          <Database class="h-4 w-4" /><span class="flex-1 truncate">{{ item.name }}</span>
          <Upload class="h-3.5 w-3.5" />
        </button>
        <p
          v-if="notice"
          class="notice"
          :class="notice.kind === 'error' ? 'notice-error' : 'notice-success'"
        >
          {{ notice.text }}
        </p>
        <button class="danger-button w-full" :disabled="sessionBusy" @click="clearMemory">
          <Trash2 class="h-4 w-4" />{{ t('sessions.clear') }}
        </button>

        <div
          v-if="confirmOpen"
          class="modal-backdrop"
          role="dialog"
          aria-modal="true"
          :aria-label="t('sessions.clearConfirmTitle')"
          @click="cancelClearMemory"
        >
          <div class="editor-dialog" @click.stop>
            <h3 class="text-sm font-semibold text-(--text-primary)">
              {{ t('sessions.clearConfirmTitle') }}
            </h3>
            <p class="mt-2 text-xs text-(--text-secondary)">
              {{ t('sessions.clearConfirm') }}
            </p>
            <div class="mt-4 flex justify-end gap-1">
              <button class="editor-button" @click="cancelClearMemory">
                {{ t('common.cancel') }}
              </button>
              <button class="danger-button" @click="confirmClearMemory">
                <Trash2 class="h-4 w-4" />{{ t('sessions.clear') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>
  </aside>
</template>
