<script setup lang="ts">
import Modal from './Modal.vue'
import { useLaunchStore } from '@/stores/launch'
import { useErrorsStore } from '@/stores/errors'
import { api } from '@/api/tauri'

const launch = useLaunchStore()
const errors = useErrorsStore()

function formatSize(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.ceil(bytes / 1024)} KB`
}
</script>

<template>
  <Modal :open="!!launch.quarantined" :title="$t('quarantine.title')" @close="launch.quarantined = null">
    <p class="mb-3 text-sm text-muted">{{ $t('quarantine.body') }}</p>
    <ul class="max-h-48 overflow-auto rounded-lg border border-line text-sm">
      <li v-for="file in launch.quarantined?.files" :key="file.name" class="flex justify-between border-b border-line px-3 py-1.5 last:border-0">
        <span class="truncate">{{ file.name }}</span>
        <span class="ml-3 shrink-0 text-muted">{{ formatSize(file.size) }}</span>
      </li>
    </ul>
    <template #actions>
      <button class="btn-ghost" @click="api.openFolder('quarantine', launch.quarantined?.eventId).catch(errors.report)">{{ $t('quarantine.open') }}</button>
    </template>
  </Modal>
</template>
