<script setup lang="ts">
import { ref } from 'vue'
import IconDiscord from './icons/IconDiscord.vue'
import { useDiscordStore } from '@/stores/discord'
import { useErrorsStore } from '@/stores/errors'
import { toAppError } from '@/api/tauri'
import { errorKey } from '@/i18n'

const props = defineProps<{ inDialog?: boolean }>()
const emit = defineEmits<{ loggedIn: [] }>()
const discord = useDiscordStore()
const errors = useErrorsStore()
const message = ref<string | null>(null)

async function login() {
  if (discord.waiting) return
  message.value = null
  try {
    const outcome = await discord.login()
    if (outcome.status === 'loggedIn') emit('loggedIn')
    else if (outcome.status === 'denied') message.value = 'accounts.discordDenied'
    else if (outcome.status === 'timedOut') message.value = 'accounts.discordTimedOut'
  } catch (err) {
    message.value = errorKey(toAppError(err).code)
  }
}
</script>

<template>
  <div>
    <div class="flex min-h-11 items-center gap-2">
      <button
        class="h-11 flex-1 aria-disabled:pointer-events-none aria-disabled:opacity-60"
        :class="props.inDialog ? 'btn-danger' : 'btn-ghost'"
        :aria-disabled="discord.waiting"
        @click="login"
      >
        <IconDiscord class="h-4 w-4 shrink-0" :class="props.inDialog ? '' : 'text-[#5865F2]'" />
        {{ discord.waiting ? $t('accounts.waitingDiscord') : $t('accounts.loginDiscord') }}
      </button>
      <button v-if="discord.waiting && !props.inDialog" class="btn-ghost h-11" @click="discord.cancel().catch(errors.report)">
        {{ $t('common.cancel') }}
      </button>
    </div>
    <p v-if="message" role="alert" class="mt-2 text-xs text-danger">{{ $t(message) }}</p>
  </div>
</template>
