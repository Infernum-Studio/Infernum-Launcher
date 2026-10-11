<script setup lang="ts">
import Modal from './Modal.vue'
import { useLaunchStore } from '@/stores/launch'
import { useErrorsStore } from '@/stores/errors'
import { api } from '@/api/tauri'

const launch = useLaunchStore()
const errors = useErrorsStore()
</script>

<template>
  <Modal :open="!!launch.crashedLog" :title="$t('crash.title')" @close="launch.crashedLog = null">
    <p class="text-sm text-muted">{{ $t('crash.body') }}</p>
    <template #actions>
      <button class="btn-primary" @click="api.openFolder('logs', launch.crashedLog?.eventId).catch(errors.report)">{{ $t('crash.openLogs') }}</button>
    </template>
  </Modal>
</template>
