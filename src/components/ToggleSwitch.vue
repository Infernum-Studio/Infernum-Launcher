<script setup lang="ts">
const model = defineModel<boolean>({ required: true })
defineProps<{ label: string; hint?: string }>()
</script>

<template>
  <label class="flex cursor-pointer items-center justify-between gap-6 py-3">
    <span>
      <span class="block text-sm font-medium">{{ label }}</span>
      <span v-if="hint" class="mt-0.5 block text-xs text-faint">{{ hint }}</span>
    </span>
    <input v-model="model" type="checkbox" class="peer sr-only" />
    <span class="track relative h-6 w-11 shrink-0 rounded-full transition peer-focus-visible:ring-2 peer-focus-visible:ring-ember/50" :class="model ? 'on' : ''">
      <span class="knob absolute top-1 left-1 h-4 w-4 rounded-full bg-white transition-transform" :class="model ? 'translate-x-5' : ''" />
    </span>
  </label>
</template>

<style scoped>
.track {
  background: rgb(255 255 255 / 0.12);
  box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.06);
}
.track.on {
  background: linear-gradient(135deg, var(--color-ember), var(--color-accent));
}
.knob {
  box-shadow: 0 1px 4px rgb(0 0 0 / 0.4);
}
</style>
