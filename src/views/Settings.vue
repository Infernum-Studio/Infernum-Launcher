<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import RamSlider from '@/components/RamSlider.vue'
import ToggleSwitch from '@/components/ToggleSwitch.vue'
import SettingRow from '@/components/SettingRow.vue'
import { useSettingsStore } from '@/stores/settings'
import { useEventsStore } from '@/stores/events'
import { useErrorsStore } from '@/stores/errors'
import { api, toAppError, type SettingsPatch } from '@/api/tauri'
import { errorKey } from '@/i18n'

const store = useSettingsStore()
const events = useEventsStore()
const errors = useErrorsStore()

const ram = ref(store.settings?.ramMb ?? 2048)
const jvmArgs = ref(store.settings?.jvmArgs ?? '')
const jvmError = ref<string | null>(null)
const saved = ref(false)
let ramTimer: ReturnType<typeof setTimeout> | undefined
let savedTimer: ReturnType<typeof setTimeout> | undefined

async function save(patch: SettingsPatch) {
  await store.update(patch)
  saved.value = true
  clearTimeout(savedTimer)
  savedTimer = setTimeout(() => (saved.value = false), 1400)
}

watch(ram, (value) => {
  clearTimeout(ramTimer)
  ramTimer = setTimeout(() => save({ ramMb: value }).catch(errors.report), 350)
})

const toggle = (key: 'closeOnLaunch' | 'logReports') =>
  computed({
    get: () => store.settings?.[key] ?? false,
    set: (value: boolean) => save({ [key]: value }).catch(errors.report),
  })
const closeOnLaunch = toggle('closeOnLaunch')
const logReports = toggle('logReports')

async function saveJvmArgs() {
  jvmError.value = null
  try {
    await save({ jvmArgs: jvmArgs.value })
  } catch (err) {
    jvmError.value = errorKey(toAppError(err).code)
  }
}

</script>

<template>
  <div v-if="store.settings && store.system" class="mx-auto max-w-3xl px-8 pt-16 pb-10">
    <div class="flex items-end justify-between">
      <h1 class="font-display text-[2.6rem] leading-none font-extrabold uppercase">{{ $t('settings.title') }}</h1>
      <Transition name="fade">
        <span v-if="saved" class="flex items-center gap-1.5 text-sm text-ok">
          <svg viewBox="0 0 16 16" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M3 8.5l3 3 7-7" /></svg>
          {{ $t('settings.saved') }}
        </span>
      </Transition>
    </div>

    <div class="glass mt-8 rounded-2xl px-7">
      <SettingRow :title="$t('settings.ram')" :hint="$t('settings.ramHint')">
        <RamSlider
          v-model="ram"
          :min="store.system.minRamMb"
          :max="store.system.maxRamMb"
          :total="store.system.totalRamMb"
          :warn-below="events.selected?.ram?.minMb"
        />
      </SettingRow>

      <SettingRow :title="$t('settings.whilePlaying')">
        <div class="-my-3 divide-y divide-white/6">
          <ToggleSwitch v-model="closeOnLaunch" :label="$t('settings.closeOnLaunch')" />
          <ToggleSwitch v-model="logReports" :label="$t('settings.logReports')" :hint="$t('settings.logReportsHint')" />
        </div>
      </SettingRow>

      <SettingRow :title="$t('settings.jvmArgs')" :hint="$t('settings.jvmArgsHint')">
        <div class="flex gap-2">
          <input id="jvm" v-model="jvmArgs" class="field font-mono text-xs" spellcheck="false" placeholder="-XX:+UseG1GC" :aria-label="$t('settings.jvmArgs')" />
          <button class="btn-ghost shrink-0" @click="saveJvmArgs">{{ $t('common.save') }}</button>
        </div>
        <p v-if="jvmError" class="mt-2 text-xs text-danger">{{ $t(jvmError) }}</p>
      </SettingRow>

      <SettingRow :title="$t('settings.folders')">
        <div class="flex flex-wrap gap-2">
          <template v-if="events.selected">
            <button class="btn-ghost" @click="api.openFolder('game', events.selected.id).catch(errors.report)">{{ $t('event.openGameFolder') }}</button>
            <button class="btn-ghost" @click="api.openFolder('logs', events.selected.id).catch(errors.report)">{{ $t('event.openLogs') }}</button>
          </template>
          <button class="btn-ghost" @click="api.openFolder('launcherLogs').catch(errors.report)">{{ $t('settings.launcherLogs') }}</button>
        </div>
      </SettingRow>
    </div>

    <footer class="mt-6 flex items-start justify-between gap-6 text-xs leading-relaxed text-faint">
      <p>
        © 2026 Infernum Studios<br />
        {{ $t('settings.notAffiliated') }}
      </p>
      <p class="shrink-0">{{ $t('settings.version', { version: store.system.version }) }}</p>
    </footer>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
