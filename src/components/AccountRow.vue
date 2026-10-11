<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import SkinHead from './SkinHead.vue'
import { useAccountsStore } from '@/stores/accounts'
import { useErrorsStore } from '@/stores/errors'
import { useSkinStore } from '@/stores/skin'
import type { Account } from '@/api/tauri'

const props = defineProps<{ account: Account }>()
const accounts = useAccountsStore()
const errors = useErrorsStore()
const skins = useSkinStore()
const active = computed(() => accounts.activeId === props.account.id)
const head = ref<string | null>(null)

onMounted(async () => {
  try {
    head.value = (await skins.fetchSkin(props.account.id)).dataUrl
  } catch {
    head.value = null
  }
})

const setActive = () => accounts.setActive(props.account.id).catch(errors.report)
const remove = () => accounts.remove(props.account.id).catch(errors.report)
</script>

<template>
  <div class="row flex items-center gap-3 rounded-xl border p-2.5 pr-3" :class="active ? 'is-active' : 'border-line bg-black/15'">
    <span class="grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded-lg bg-white/5">
      <SkinHead :data-url="head" :size="30" />
    </span>
    <div class="min-w-0 flex-1">
      <div class="truncate text-sm font-semibold">{{ account.name }}</div>
      <div class="text-xs text-faint">
        {{ account.kind === 'microsoft' ? $t('accounts.signedIn') : $t('accounts.offline') }}
      </div>
    </div>
    <span v-if="active" class="flex items-center gap-1.5 text-xs font-medium text-ember"><span class="h-1.5 w-1.5 rounded-full bg-ember" />{{ $t('accounts.active') }}</span>
    <button v-else class="btn-ghost !py-1.5" @click="setActive">{{ $t('accounts.use') }}</button>
    <button class="btn !px-2.5 !py-1.5 text-faint hover:bg-white/5 hover:text-danger" @click="remove">
      {{ account.kind === 'microsoft' ? $t('accounts.signOut') : $t('common.remove') }}
    </button>
  </div>
</template>

<style scoped>
.is-active {
  border-color: rgb(255 106 61 / 0.4);
  background: linear-gradient(90deg, rgb(224 49 58 / 0.14), rgb(255 255 255 / 0.02));
}
</style>
