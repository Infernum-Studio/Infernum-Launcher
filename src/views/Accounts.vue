<script setup lang="ts">
import { computed } from 'vue'
import AccountForm from '@/components/AccountForm.vue'
import MicrosoftLogin from '@/components/MicrosoftLogin.vue'
import AccountRow from '@/components/AccountRow.vue'
import SettingRow from '@/components/SettingRow.vue'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'

const accounts = useAccountsStore()
const settings = useSettingsStore()
const microsoft = computed(() => accounts.accounts.filter((a) => a.kind === 'microsoft'))
const offline = computed(() => accounts.accounts.filter((a) => a.kind === 'offline'))
</script>

<template>
  <div class="mx-auto max-w-3xl px-8 pt-16 pb-10">
    <h1 class="font-display text-[2.6rem] leading-none font-extrabold uppercase">{{ $t('accounts.title') }}</h1>

    <div class="glass mt-8 rounded-2xl px-7">
      <SettingRow v-if="settings.system?.msaEnabled" :title="$t('accounts.microsoftTitle')" :hint="microsoft.length ? undefined : $t('accounts.microsoftHint')">
        <div v-if="microsoft.length" class="flex flex-col gap-2">
          <AccountRow v-for="a in microsoft" :key="a.id" :account="a" />
          <MicrosoftLogin another class="mt-1" />
        </div>
        <MicrosoftLogin v-else />
      </SettingRow>

      <SettingRow :title="$t('accounts.offlineTitle')">
        <div v-if="offline.length" class="mb-4 flex flex-col gap-2">
          <AccountRow v-for="a in offline" :key="a.id" :account="a" />
        </div>
        <AccountForm />
      </SettingRow>
    </div>
  </div>
</template>
