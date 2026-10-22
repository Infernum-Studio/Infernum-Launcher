<script setup lang="ts">
import Modal from './Modal.vue'
import DiscordLogin from './DiscordLogin.vue'
import { useLaunchStore } from '@/stores/launch'
import { useDiscordStore } from '@/stores/discord'
import { useErrorsStore } from '@/stores/errors'

const launch = useLaunchStore()
const discord = useDiscordStore()
const errors = useErrorsStore()

function close() {
  if (discord.waiting) discord.cancel().catch(errors.report)
  launch.needsDiscord = null
}

function retry() {
  const pending = launch.needsDiscord
  launch.needsDiscord = null
  if (pending) launch.play(pending.eventId)
}
</script>

<template>
  <Modal :open="!!launch.needsDiscord" :title="$t('play.discordTitle')" :close-label="$t('common.cancel')" @close="close">
    <p class="mb-5 text-sm text-muted">{{ $t(launch.needsDiscord?.expired ? 'play.discordExpired' : 'play.discordBody') }}</p>
    <DiscordLogin in-dialog @logged-in="retry" />
  </Modal>
</template>
