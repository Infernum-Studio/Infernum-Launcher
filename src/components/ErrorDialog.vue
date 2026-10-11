<script setup lang="ts">
import { ref } from 'vue'
import Modal from './Modal.vue'
import { errorText } from '@/i18n'
import type { AppError } from '@/api/tauri'

defineProps<{ error: AppError | null }>()
defineEmits<{ close: [] }>()
const showDetails = ref(false)
</script>

<template>
  <Modal :open="!!error" :title="error ? errorText(error) : ''" @close="$emit('close')">
    <template v-if="error">
      <button class="text-xs text-muted underline" @click="showDetails = !showDetails">{{ $t('common.details') }}</button>
      <pre v-if="showDetails" class="mt-2 max-h-40 overflow-auto rounded-lg bg-black/30 p-3 font-mono text-xs whitespace-pre-wrap text-muted select-text">{{ error.code }}: {{ error.message }}</pre>
    </template>
  </Modal>
</template>
