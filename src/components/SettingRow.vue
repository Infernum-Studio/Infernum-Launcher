<script setup lang="ts">
defineProps<{ title: string; hint?: string }>()
</script>

<template>
  <div class="row grid grid-cols-[minmax(0,13rem)_minmax(0,1fr)] gap-x-10 gap-y-3 py-6">
    <div>
      <h2 class="text-[15px] font-semibold">{{ title }}</h2>
      <p v-if="hint" class="mt-1 text-[13px] leading-snug text-muted">{{ hint }}</p>
    </div>
    <div class="min-w-0">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.row + .row {
  border-top: 1px solid rgb(255 255 255 / 0.07);
}
</style>
