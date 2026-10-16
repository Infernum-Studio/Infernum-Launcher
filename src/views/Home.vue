<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import SkinViewer from '@/components/SkinViewer.vue'
import ErrorDialog from '@/components/ErrorDialog.vue'
import Modal from '@/components/Modal.vue'
import { useEventsStore } from '@/stores/events'
import { useLaunchStore } from '@/stores/launch'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'
import { useSkinStore } from '@/stores/skin'
import { useErrorsStore } from '@/stores/errors'
import { api, eventName } from '@/api/tauri'
import { gb } from '@/format'

const events = useEventsStore()
const launch = useLaunchStore()
const accounts = useAccountsStore()
const settings = useSettingsStore()
const skin = useSkinStore()
const errors = useErrorsStore()
const { locale, t } = useI18n()

const newsOpen = ref(false)
const pickerOpen = ref(false)
const event = computed(() => events.selected)
const title = computed(() => (event.value ? eventName(event.value, locale.value) : null))
const nameOf = (id: string) => {
  const e = events.byId(id)
  return (e && eventName(e, locale.value)) ?? id
}

const accent = computed(() => {
  const a = event.value?.art.accent
  return a && /^#[0-9a-f]{6}$/i.test(a) ? a : null
})

function selectEvent(id: string) {
  events.select(id)
  pickerOpen.value = false
}

const pickerRoot = ref<HTMLElement | null>(null)
function onPointer(e: PointerEvent) {
  if (!pickerRoot.value?.contains(e.target as Node)) pickerOpen.value = false
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') pickerOpen.value = false
}
function unlisten() {
  document.removeEventListener('pointerdown', onPointer)
  document.removeEventListener('keydown', onKey)
}
watch(pickerOpen, (open) => {
  unlisten()
  if (open) {
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
  }
})
onBeforeUnmount(unlisten)

const isThisEvent = computed(() => !!event.value && launch.eventId === event.value.id)
const phase = computed(() => (isThisEvent.value ? launch.phase : 'idle'))
const busy = computed(() => ['checking', 'installing', 'launching'].includes(phase.value))
const playLabel = computed(() => {
  if (event.value?.status === 'ended') return 'play.ended'
  if (!accounts.active) return 'play.noAccount'
  return {
    idle: 'play.play',
    checking: 'play.checking',
    installing: 'play.installing',
    launching: 'play.launching',
    running: 'play.running',
  }[phase.value]
})
const playDisabled = computed(
  () =>
    !event.value ||
    event.value.status === 'ended' ||
    !accounts.active ||
    (launch.phase !== 'idle' && !isThisEvent.value) ||
    phase.value === 'launching',
)
const lowRam = computed(() => {
  const min = event.value?.ram?.minMb
  const ram = settings.settings?.ramMb ?? 0
  return min && ram < min ? { ram: gb(ram), min: gb(min) } : null
})
const stepText = computed(() =>
  launch.step === 'minecraft' && launch.detail
    ? t('progress.minecraftLoader', { loader: launch.detail })
    : t(`progress.${launch.step}`),
)
const error = computed(() => (!launch.eventId || isThisEvent.value ? launch.error : null))

const confirmStop = ref(false)
watch(phase, (p) => {
  if (p !== 'running') confirmStop.value = false
})

function play() {
  if (!event.value) return
  if (phase.value === 'idle') launch.play(event.value.id)
  else if (phase.value === 'running') confirmStop.value = true
  else launch.stop().catch(errors.report)
}

function stopGame() {
  confirmStop.value = false
  launch.stop().catch(errors.report)
}

</script>

<template>
  <div class="relative flex h-full flex-col" :style="accent ? { '--event-accent': accent } : {}">
    <div class="relative flex min-h-0 flex-1">
      <section class="z-10 flex min-w-0 flex-1 flex-col justify-center pt-12 pr-6 pb-6 pl-[9%]">
        <template v-if="event">
          <div ref="pickerRoot" class="relative">
            <component
              :is="events.events.length > 1 ? 'button' : 'div'"
              class="group flex items-end gap-4 text-left"
              :aria-expanded="events.events.length > 1 ? pickerOpen : undefined"
              @click="events.events.length > 1 && (pickerOpen = !pickerOpen)"
            >
              <img v-if="event.art.logo" :src="event.art.logo" :alt="title ?? event.id" class="max-h-40 max-w-[min(520px,100%)] object-contain object-left" />
              <h1 v-else class="title font-display">{{ title ?? '' }}</h1>
              <svg
                v-if="events.events.length > 1"
                viewBox="0 0 24 24"
                class="mb-3 h-7 w-7 shrink-0 text-muted transition group-hover:text-text"
                :class="{ 'rotate-180': pickerOpen }"
                fill="none"
                stroke="currentColor"
                stroke-width="2.2"
              ><path d="M6 9l6 6 6-6" /></svg>
            </component>
            <Transition name="pop">
              <ul v-if="pickerOpen" class="glass-strong absolute top-full left-0 z-30 mt-3 min-w-72 rounded-xl p-1.5">
                <li v-for="e in events.events" :key="e.id">
                  <button
                    class="flex w-full items-center justify-between gap-6 rounded-lg px-3.5 py-2.5 text-left transition-colors hover:bg-white/6"
                    :class="{ 'text-ember': e.id === event.id }"
                    @click="selectEvent(e.id)"
                  >
                    <span class="font-display text-xl font-bold uppercase">{{ nameOf(e.id) }}</span>
                    <span v-if="e.status === 'ended'" class="text-xs text-faint">{{ $t('status.ended') }}</span>
                  </button>
                </li>
              </ul>
            </Transition>
          </div>
          <p v-if="event.status === 'ended'" class="mt-4 text-sm text-muted">{{ $t('home.endedHint') }}</p>
          <p v-if="lowRam" class="mt-6 max-w-sm text-xs text-warn">{{ $t('event.ramBelowMin', lowRam) }}</p>
        </template>

        <p v-else-if="events.loading" class="title font-display text-faint">{{ $t('common.loading') }}</p>

        <div v-else-if="events.error" class="max-w-md">
          <h1 class="title stacked font-display text-muted">
            <span v-for="w in $t('home.loadFailedTitle').split(' ')" :key="w" class="block">{{ w }}</span>
          </h1>
          <p class="mt-4 text-sm text-muted">{{ $t('home.loadFailed') }}</p>
          <button class="text-action mt-7" @click="events.load()">{{ $t('common.retry') }}</button>
        </div>

        <div v-else class="max-w-md">
          <h1 class="title stacked font-display text-muted">
            <span v-for="w in $t('home.noEventsTitle').split(' ')" :key="w" class="block">{{ w }}</span>
          </h1>
          <p class="mt-4 text-sm text-muted">{{ $t('home.noEvents') }}</p>
          <button class="text-action mt-7" @click="api.openLink('discord').catch(errors.report)">{{ $t('home.joinDiscord') }}</button>
        </div>
      </section>

      <section v-if="accounts.active && skin.skin" class="relative flex w-[300px] shrink-0 items-end justify-center">
        <div class="pedestal" />
        <SkinViewer :data-url="skin.skin.dataUrl" :slim="skin.skin.slim" :width="260" :height="390" class="relative" />
      </section>
    </div>

    <footer class="relative z-20 grid h-[92px] shrink-0 grid-cols-[1fr_auto_1fr] items-center px-10">
      <div class="min-w-0 pr-6">
        <template v-if="isThisEvent && busy && launch.step">
          <div class="mb-1.5 flex max-w-xs justify-between text-xs">
            <span class="truncate text-muted">{{ stepText }}</span>
            <span v-if="launch.percent !== null" class="tabular-nums">{{ Math.floor(launch.percent) }}%</span>
          </div>
          <div class="h-[3px] max-w-xs overflow-hidden rounded-full bg-white/10">
            <div v-if="launch.percent !== null" class="h-full rounded-full bg-ember transition-[width] duration-150 ease-linear" :style="{ width: `${launch.percent}%` }" />
            <div v-else class="indeterminate h-full w-1/4 rounded-full bg-ember" />
          </div>
        </template>
        <p v-else-if="phase === 'running'" class="flex items-center gap-2 text-xs font-semibold tracking-wider text-muted uppercase">
          <span class="h-1.5 w-1.5 rounded-full bg-ok" />{{ $t('home.running') }}
        </p>
        <p v-else-if="event && events.offline" class="max-w-xs text-xs text-faint">{{ $t('home.offline') }}</p>
      </div>

      <button class="news flex flex-col items-center gap-1" :class="{ open: newsOpen }" @click="newsOpen = !newsOpen">
        <svg viewBox="0 0 24 12" class="chev h-3 w-6" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M3 10l9-7 9 7" /></svg>
        {{ $t('home.news') }}
      </button>

      <div class="flex items-center justify-end">
        <button class="play" :class="{ running: phase === 'running', busy }" :disabled="playDisabled" @click="play">
          <span v-if="busy" class="spinner" />
          {{ $t(playLabel) }}
        </button>
      </div>
    </footer>

    <Transition name="pop">
      <section v-if="newsOpen" class="glass-strong absolute bottom-[100px] left-1/2 z-30 w-[min(520px,calc(100%-4rem))] -translate-x-1/2 rounded-2xl p-6">
        <h2 class="font-display text-2xl font-bold uppercase">{{ $t('home.news') }}</h2>
        <p class="mt-2 text-sm text-muted">{{ $t('home.noNews') }}</p>
      </section>
    </Transition>

    <ErrorDialog :error="error" @close="launch.error = null" />
    <Modal :open="confirmStop" :title="$t('play.stopTitle')" :close-label="$t('common.cancel')" @close="confirmStop = false">
      <p class="text-sm text-muted">{{ $t('play.stopBody') }}</p>
      <template #actions>
        <button class="btn-primary" @click="stopGame">{{ $t('play.stopConfirm') }}</button>
      </template>
    </Modal>
  </div>
</template>

<style scoped>
.title {
  font-size: clamp(4rem, 9vw, 7.5rem);
  font-weight: 800;
  line-height: 0.86;
  letter-spacing: 0.005em;
  text-transform: uppercase;
  text-wrap: balance;
  color: var(--color-text);
  text-shadow: 0 6px 40px color-mix(in srgb, var(--event-accent, var(--color-glow)) 40%, transparent);
}
.stacked {
  display: inline-flex;
  flex-direction: column;
  align-items: center;
}
.text-action {
  font-family: var(--font-display);
  font-size: 1.5rem;
  font-weight: 800;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--color-text);
  text-shadow: 0 2px 16px color-mix(in srgb, var(--event-accent, var(--color-glow)) 50%, transparent);
  transition: color 0.12s;
}
.text-action:hover {
  color: var(--color-ember-soft);
}
.pedestal {
  position: absolute;
  bottom: 34px;
  width: 210px;
  height: 36px;
  border-radius: 50%;
  background: radial-gradient(closest-side, rgb(255 110 60 / 0.45), transparent);
}
.news {
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--color-text);
  opacity: 0.85;
  transition: opacity 0.12s, color 0.12s;
}
.news:hover,
.news.open {
  opacity: 1;
  color: var(--color-ember);
}
.news .chev {
  transition: transform 0.18s;
}
.news.open .chev {
  transform: rotate(180deg);
}
.play {
  display: inline-flex;
  align-items: center;
  gap: 0.6rem;
  font-family: var(--font-display);
  font-size: 2.6rem;
  font-weight: 800;
  line-height: 1;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: #fff;
  text-shadow: 0 2px 18px color-mix(in srgb, var(--event-accent, var(--color-glow)) 60%, transparent);
  transition: color 0.12s, text-shadow 0.12s, transform 0.08s;
}
.play:hover:not(:disabled) {
  color: var(--color-ember-soft);
  text-shadow: 0 2px 26px color-mix(in srgb, var(--event-accent, var(--color-glow)) 90%, transparent);
}
.play:active:not(:disabled) {
  transform: translateY(1px);
}
.play:disabled {
  cursor: not-allowed;
  color: rgb(246 238 235 / 0.4);
  text-shadow: none;
}
.play.busy,
.play.busy:disabled {
  color: var(--color-text);
  font-size: 1.6rem;
  text-shadow: none;
}
.play.running {
  color: var(--color-ember-soft);
}
.spinner {
  width: 15px;
  height: 15px;
  border-radius: 50%;
  border: 2px solid rgb(255 255 255 / 0.25);
  border-top-color: currentColor;
  animation: spin 0.8s linear infinite;
}
.indeterminate {
  animation: slide 1.2s ease-in-out infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
@keyframes slide {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(400%);
  }
}
.pop-enter-active,
.pop-leave-active {
  transition: opacity 0.14s ease, transform 0.14s ease;
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
}
</style>
