<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{ min: number; max: number; total: number; warnBelow?: number }>()
const model = defineModel<number>({ required: true })
const toGb = (mb: number) => Math.round((mb / 1024) * 10) / 10
const minGb = computed(() => Math.ceil((props.min / 1024) * 10) / 10)
const maxGb = computed(() => Math.floor((props.max / 1024) * 10) / 10)
const valueGb = computed({
  get: () => toGb(model.value),
  set: (gb: number) => (model.value = Math.round(gb * 1024)),
})
const fill = computed(() => `${((valueGb.value - minGb.value) / Math.max(0.1, maxGb.value - minGb.value)) * 100}%`)
const gb = (value: number) => value.toFixed(1)
</script>

<template>
  <div>
    <div class="mb-4 flex items-end justify-between">
      <div class="font-display text-4xl font-bold tabular-nums">
        {{ gb(valueGb) }}<span class="ml-1 text-base text-muted">GB</span>
      </div>
      <span class="text-xs text-faint">{{ $t('settings.ramOf', { total: gb(toGb(props.total)) }) }}</span>
    </div>
    <input v-model.number="valueGb" type="range" class="range w-full" :min="minGb" :max="maxGb" step="0.1" :aria-label="$t('settings.ram')" :style="{ '--fill': fill }" />
    <div class="mt-2 flex justify-between text-[11px] text-faint tabular-nums">
      <span>{{ gb(minGb) }} GB</span>
      <span>{{ gb(maxGb) }} GB</span>
    </div>
    <p v-if="props.warnBelow && model < props.warnBelow" class="mt-3 text-xs text-warn">
      {{ $t('event.ramBelowMin', { ram: gb(valueGb), min: gb(toGb(props.warnBelow)) }) }}
    </p>
  </div>
</template>
