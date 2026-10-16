<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import SkinHead from './SkinHead.vue'
import IconDiscord from './icons/IconDiscord.vue'
import IconX from './icons/IconX.vue'
import { useAccountsStore } from '@/stores/accounts'
import { useSkinStore } from '@/stores/skin'
import { useErrorsStore } from '@/stores/errors'
import { useEventsStore } from '@/stores/events'
import { api, eventName } from '@/api/tauri'

const accounts = useAccountsStore()
const skin = useSkinStore()
const errors = useErrorsStore()
const events = useEventsStore()
const route = useRoute()
const router = useRouter()
const { locale } = useI18n()

const eventsOpen = ref(true)
const inEvents = computed(() => route.name === 'home' || route.name === 'event')

function openEvents() {
  if (inEvents.value) {
    eventsOpen.value = !eventsOpen.value
    return
  }
  eventsOpen.value = true
  router.push(events.selected ? `/event/${events.selected.id}` : '/')
}

const homeIcon = 'M4 10.5 12 4l8 6.5V20h-5.5v-5.5h-5V20H4z'
const links = [
  { to: '/accounts', key: 'nav.accounts', icon: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm-7 8.5c.6-3.4 3.5-5.5 7-5.5s6.4 2.1 7 5.5' },
  { to: '/settings', key: 'nav.settings', icon: 'M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5' },
]
</script>

<template>
  <aside class="glass relative z-10 m-2.5 mr-0 flex w-[200px] shrink-0 flex-col rounded-2xl px-2.5 pt-5 pb-2.5">
    <div class="px-3 pb-7" data-tauri-drag-region>
      <div class="pointer-events-none font-display text-[26px] leading-none font-extrabold tracking-[0.06em]">INFERNUM</div>
      <div class="pointer-events-none mt-1 text-[11px] font-semibold tracking-[0.34em] text-muted">LAUNCHER</div>
    </div>

    <nav class="flex flex-col gap-0.5">
      <button
        class="nav-item relative flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-[13.5px] font-medium"
        :class="{ active: inEvents }"
        :aria-expanded="events.events.length ? eventsOpen : undefined"
        @click="openEvents"
      >
        <svg viewBox="0 0 24 24" class="h-[17px] w-[17px] shrink-0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path :d="homeIcon" />
        </svg>
        {{ $t('nav.home') }}
      </button>
      <div v-if="events.events.length" class="event-list" :class="{ open: eventsOpen }">
        <ul class="overflow-hidden">
          <li v-for="e in events.events" :key="e.id">
            <RouterLink :to="`/event/${e.id}`" class="sub-item" :class="{ active: inEvents && e.id === events.selected?.id }" :tabindex="eventsOpen ? 0 : -1">
              <span class="truncate">{{ eventName(e, locale) ?? e.id }}</span>
              <span v-if="e.status === 'ended'" class="shrink-0 text-[10.5px] text-faint">{{ $t('status.ended') }}</span>
            </RouterLink>
          </li>
        </ul>
      </div>
      <RouterLink
        v-for="link in links"
        :key="link.to"
        :to="link.to"
        class="nav-item relative flex items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] font-medium"
        exact-active-class="active"
      >
        <svg viewBox="0 0 24 24" class="h-[17px] w-[17px] shrink-0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path :d="link.icon" />
        </svg>
        {{ $t(link.key) }}
      </RouterLink>
    </nav>

    <div class="mt-auto flex items-center justify-center gap-1 pb-3">
      <button class="social" title="Discord" aria-label="Discord" @click="api.openLink('discord').catch(errors.report)"><IconDiscord class="h-[15px] w-[15px]" /></button>
      <button class="social" title="X" aria-label="X" @click="api.openLink('twitter').catch(errors.report)"><IconX class="h-[13px] w-[13px]" /></button>
    </div>

    <RouterLink to="/accounts" class="flex items-center gap-2.5 rounded-xl px-2 py-2 transition hover:bg-white/5">
      <span class="grid h-8 w-8 shrink-0 place-items-center overflow-hidden rounded-md bg-white/5">
        <SkinHead v-if="accounts.active" :data-url="skin.skin?.dataUrl ?? null" :size="26" />
      </span>
      <span class="min-w-0">
        <span class="block truncate text-[13px] font-semibold">{{ accounts.active?.name ?? $t('play.noAccount') }}</span>
        <span v-if="accounts.active" class="block text-[11px] text-faint">{{ $t(`accounts.${accounts.active.kind}`) }}</span>
      </span>
    </RouterLink>
  </aside>
</template>

<style scoped>
.nav-item {
  color: var(--color-muted);
  transition: color 0.12s, background 0.12s;
}
.nav-item:hover {
  color: var(--color-text);
}
.nav-item.active {
  color: #fff;
  background: rgb(255 255 255 / 0.07);
}
.nav-item::before {
  content: '';
  position: absolute;
  left: 0;
  top: 50%;
  width: 3px;
  height: 16px;
  border-radius: 0 3px 3px 0;
  background: var(--color-ember);
  box-shadow: 0 0 10px var(--color-ember);
  transform: translateY(-50%) scaleY(0);
  transition: transform 0.18s ease;
}
.nav-item.active::before {
  transform: translateY(-50%) scaleY(1);
}
.event-list {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows 0.2s ease;
}
.event-list.open {
  grid-template-rows: 1fr;
}
.sub-item {
  position: relative;
  display: flex;
  width: calc(100% - 20px);
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-left: 20px;
  padding: 6px 10px 6px 21px;
  border-left: 1px solid var(--color-line);
  font-size: 12.5px;
  font-weight: 500;
  text-align: left;
  color: var(--color-muted);
  transition: color 0.12s;
}
.event-list.open ul {
  padding: 2px 0 4px;
}
.sub-item:hover {
  color: var(--color-text);
}
.sub-item.active {
  color: #fff;
}
.sub-item::before {
  content: '';
  position: absolute;
  left: -1px;
  top: 50%;
  width: 1px;
  height: 14px;
  background: var(--color-ember);
  box-shadow: 0 0 8px var(--color-ember);
  transform: translateY(-50%) scaleY(0);
  transition: transform 0.18s ease;
}
.sub-item.active::before {
  transform: translateY(-50%) scaleY(1);
}
.social {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 8px;
  color: var(--color-faint);
  transition: color 0.12s, background 0.12s;
}
.social:hover {
  color: var(--color-text);
  background: rgb(255 255 255 / 0.06);
}
</style>
