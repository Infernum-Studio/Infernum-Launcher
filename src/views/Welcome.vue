<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import RamSlider from '@/components/RamSlider.vue'
import AccountForm from '@/components/AccountForm.vue'
import MicrosoftLogin from '@/components/MicrosoftLogin.vue'
import { useSettingsStore } from '@/stores/settings'
import { useAccountsStore } from '@/stores/accounts'
import { useErrorsStore } from '@/stores/errors'

const router = useRouter()
const settings = useSettingsStore()
const accounts = useAccountsStore()
const errors = useErrorsStore()
const step = ref(0)
const ram = ref(settings.settings?.ramMb ?? 2048)
const steps = ['ram', 'account', 'done'] as const
const current = computed(() => steps[step.value])

async function saveRam() {
  try {
    await settings.update({ ramMb: ram.value })
    step.value++
  } catch (err) {
    errors.report(err)
  }
}
async function finish() {
  try {
    await settings.update({ onboardingDone: true })
    await router.replace('/')
  } catch (err) {
    errors.report(err)
  }
}
</script>

<template>
  <div class="flex min-h-full items-center justify-center px-8 py-16">
    <div class="glass-strong w-full max-w-[460px] rounded-3xl p-8">
      <div class="font-display text-xl font-extrabold tracking-[0.18em]">INFERNUM</div>
      <div class="mt-6 flex gap-1.5">
        <span v-for="(_, i) in steps" :key="i" class="h-1 flex-1 rounded-full transition-colors" :class="i <= step ? 'bg-gradient-to-r from-ember to-accent' : 'bg-white/10'" />
      </div>

      <Transition name="step" mode="out-in">
        <section v-if="current === 'ram'" key="ram" class="mt-8">
          <h1 class="font-display text-3xl font-bold uppercase">{{ $t('welcome.title') }}</h1>
          <p class="mt-4 text-sm font-semibold">{{ $t('welcome.ramTitle') }}</p>
          <p class="mt-2 mb-6 text-sm text-muted">{{ $t('welcome.ramHint') }}</p>
          <RamSlider v-if="settings.system" v-model="ram" :min="settings.system.minRamMb" :max="settings.system.maxRamMb" :total="settings.system.totalRamMb" />
          <div class="mt-8 flex justify-end">
            <button class="btn-primary" @click="saveRam">{{ $t('common.next') }}</button>
          </div>
        </section>

        <section v-else-if="current === 'account'" key="account" class="mt-8">
          <h1 class="font-display text-3xl font-bold uppercase">{{ $t('welcome.accountTitle') }}</h1>
          <p class="mt-2 mb-6 text-sm text-muted">{{ $t('welcome.accountHint') }}</p>
          <div v-if="accounts.active" class="flex items-center justify-between rounded-xl border border-ember/40 bg-accent/10 px-4 py-3 text-sm">
            <span class="font-semibold">{{ accounts.active.name }}</span>
            <span class="text-xs text-muted">{{ $t(`accounts.${accounts.active.kind}`) }}</span>
          </div>
          <template v-else>
            <MicrosoftLogin v-if="settings.system?.msaEnabled" @added="step++" />
            <div v-if="settings.system?.msaEnabled" class="my-5 flex items-center gap-3 text-[11px] text-faint uppercase">
              <span class="h-px flex-1 bg-white/10" />{{ $t('accounts.orOffline') }}<span class="h-px flex-1 bg-white/10" />
            </div>
            <AccountForm @added="step++" />
          </template>
          <div class="mt-8 flex justify-between">
            <button class="btn-ghost" @click="step--">{{ $t('common.back') }}</button>
            <button class="btn-primary" :disabled="!accounts.active" @click="step++">{{ $t('common.next') }}</button>
          </div>
        </section>

        <section v-else key="done" class="mt-8">
          <h1 class="font-display text-3xl font-bold uppercase">{{ $t('welcome.doneTitle') }}</h1>
          <p class="mt-2 text-sm text-muted">{{ $t('welcome.doneHint') }}</p>
          <div class="mt-8 flex justify-end">
            <button class="btn-primary" @click="finish">{{ $t('common.finish') }}</button>
          </div>
        </section>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.step-enter-active,
.step-leave-active {
  transition: opacity 0.16s, transform 0.16s;
}
.step-enter-from {
  opacity: 0;
  transform: translateX(10px);
}
.step-leave-to {
  opacity: 0;
  transform: translateX(-10px);
}
</style>
