<script setup lang="ts">
import { ref } from 'vue'
import { useAccountsStore } from '@/stores/accounts'
import { toAppError } from '@/api/tauri'
import { errorKey } from '@/i18n'

defineProps<{ another?: boolean }>()
const emit = defineEmits<{ added: [] }>()
const accounts = useAccountsStore()
const busy = ref(false)
const error = ref<string | null>(null)

async function login() {
  busy.value = true
  error.value = null
  try {
    if (await accounts.loginMicrosoft()) emit('added')
  } catch (err) {
    error.value = errorKey(toAppError(err).code)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div>
    <button v-if="another" class="text-sm font-medium text-muted underline-offset-4 hover:text-text hover:underline" :disabled="busy" @click="login">
      {{ busy ? $t('accounts.waitingMicrosoft') : $t('accounts.useAnother') }}
    </button>
    <button v-else class="btn-ghost h-11 w-full" :disabled="busy" @click="login">
      <svg viewBox="0 0 21 21" class="h-4 w-4" aria-hidden="true">
        <rect x="1" y="1" width="9" height="9" fill="#f25022" />
        <rect x="11" y="1" width="9" height="9" fill="#7fba00" />
        <rect x="1" y="11" width="9" height="9" fill="#00a4ef" />
        <rect x="11" y="11" width="9" height="9" fill="#ffb900" />
      </svg>
      {{ busy ? $t('accounts.waitingMicrosoft') : $t('accounts.loginMicrosoft') }}
    </button>
    <p v-if="error" class="mt-2 text-xs text-danger">{{ $t(error) }}</p>
  </div>
</template>
