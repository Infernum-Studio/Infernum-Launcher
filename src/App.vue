<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppBackdrop from './components/AppBackdrop.vue'
import WindowControls from './components/WindowControls.vue'
import Sidebar from './components/Sidebar.vue'
import ErrorDialog from './components/ErrorDialog.vue'
import QuarantineDialog from './components/QuarantineDialog.vue'
import CrashDialog from './components/CrashDialog.vue'
import { useSettingsStore } from './stores/settings'
import { useAccountsStore } from './stores/accounts'
import { useEventsStore } from './stores/events'
import { useLaunchStore } from './stores/launch'
import { useErrorsStore } from './stores/errors'

const router = useRouter()
const route = useRoute()
const settings = useSettingsStore()
const accounts = useAccountsStore()
const events = useEventsStore()
const launch = useLaunchStore()
const errors = useErrorsStore()
const ready = ref(false)

onMounted(async () => {
  try {
    await Promise.all([settings.load(), accounts.load(), launch.listenAll()])
    if (!settings.settings?.onboardingDone) await router.replace('/welcome')
    events.load()
  } catch (err) {
    errors.report(err)
  } finally {
    ready.value = true
  }
})
</script>

<template>
  <AppBackdrop :dim-bottom="route.name === 'home'" />
  <div class="relative z-10 flex h-full">
    <Sidebar v-if="ready && settings.settings?.onboardingDone" />
    <div class="relative flex min-w-0 flex-1 flex-col">
      <div class="absolute inset-x-0 top-0 z-30 flex h-14 items-center justify-end pr-4" data-tauri-drag-region>
        <WindowControls />
      </div>
      <main v-if="ready" class="min-h-0 flex-1 overflow-y-auto">
        <RouterView v-slot="{ Component }">
          <Transition name="page" mode="out-in">
            <component :is="Component" />
          </Transition>
        </RouterView>
      </main>
    </div>
  </div>
  <ErrorDialog :error="errors.current" @close="errors.clear()" />
  <QuarantineDialog />
  <CrashDialog />
</template>

<style>
.page-enter-active,
.page-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}
.page-enter-from {
  opacity: 0;
  transform: translateY(6px);
}
.page-leave-to {
  opacity: 0;
}
</style>
