<script setup lang="ts">
import { computed, ref } from 'vue'
import { isValidNickname, useAccountsStore } from '@/stores/accounts'
import { errorKey } from '@/i18n'
import { toAppError } from '@/api/tauri'

const emit = defineEmits<{ added: [] }>()
const accounts = useAccountsStore()
const name = ref('')
const error = ref<string | null>(null)
const valid = computed(() => isValidNickname(name.value.trim()))

async function submit() {
  if (!valid.value) return
  error.value = null
  try {
    await accounts.addOffline(name.value.trim())
    name.value = ''
    emit('added')
  } catch (err) {
    error.value = errorKey(toAppError(err).code)
  }
}
</script>

<template>
  <form @submit.prevent="submit">
    <div class="flex gap-2">
      <input
        v-model="name"
        class="field"
        maxlength="16"
        autocomplete="off"
        spellcheck="false"
        :placeholder="$t('accounts.nickname')"
        :aria-label="$t('accounts.nickname')"
      />
      <button class="btn-ghost shrink-0" :disabled="!valid" type="submit">{{ $t('accounts.add') }}</button>
    </div>
    <p class="mt-2 text-xs" :class="name && !valid ? 'text-warn' : 'text-faint'">{{ $t('accounts.nicknameRule') }}</p>
    <p v-if="error" class="mt-1 text-xs text-danger">{{ $t(error) }}</p>
  </form>
</template>
