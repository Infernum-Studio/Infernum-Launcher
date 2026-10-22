<script setup lang="ts">
import { computed } from 'vue'
import AccountForm from '@/components/AccountForm.vue'
import MicrosoftLogin from '@/components/MicrosoftLogin.vue'
import AccountRow from '@/components/AccountRow.vue'
import SettingRow from '@/components/SettingRow.vue'
import DiscordLogin from '@/components/DiscordLogin.vue'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'
import { useDiscordStore } from '@/stores/discord'
import { useErrorsStore } from '@/stores/errors'

const accounts = useAccountsStore()
const settings = useSettingsStore()
const discord = useDiscordStore()
const errors = useErrorsStore()
const initial = computed(() => Array.from(discord.session?.user.name ?? '?')[0]?.toUpperCase())
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

      <SettingRow :title="$t('accounts.discordTitle')" :hint="$t('accounts.discordHint')">
        <div v-if="!discord.loaded" class="min-h-11" />
        <div v-else-if="discord.session" class="flex items-center gap-3 rounded-xl border border-line bg-black/15 p-2.5 pr-3">
          <span class="grid h-10 w-10 shrink-0 place-items-center rounded-lg bg-[#5865F2]/20 text-base font-semibold text-[#8f98ff]" aria-hidden="true">
            {{ initial }}
          </span>
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-semibold">{{ discord.session.user.name }}</div>
            <div class="text-xs text-faint">{{ $t('accounts.discordSignedIn') }}</div>
          </div>
          <button class="btn !px-2.5 !py-1.5 text-faint hover:bg-white/5 hover:text-danger" @click="discord.logout().catch(errors.report)">
            {{ $t('accounts.signOut') }}
          </button>
        </div>
        <DiscordLogin v-else />
      </SettingRow>
    </div>
  </div>
</template>
