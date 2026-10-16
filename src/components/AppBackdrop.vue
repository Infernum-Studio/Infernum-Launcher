<script setup lang="ts">
import { computed } from 'vue'
import { useEventsStore } from '@/stores/events'
import defaultArt from '@/assets/background.webp'

const props = defineProps<{ dimBottom?: boolean }>()

const events = useEventsStore()
const art = computed(() => events.selected?.art.background ?? null)
const showArt = computed(() => !!art.value || !!props.dimBottom)
</script>

<template>
  <div class="backdrop pointer-events-none" aria-hidden="true">
    <img :src="art ?? defaultArt" alt="" draggable="false" class="art absolute inset-0 h-full w-full object-cover" :class="{ on: showArt }" />
    <div class="shade over-art" />
    <div class="shade plain" :class="{ on: !showArt }" />
    <div class="bottom" :class="{ on: dimBottom }" />
    <div class="grain" />
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  overflow: hidden;
  z-index: 0;
  background: #0c0809;
}
.art,
.shade.plain {
  opacity: 0;
  transition: opacity 0.25s ease;
}
.art.on,
.shade.plain.on {
  opacity: 1;
}
.shade {
  position: absolute;
  inset: 0;
}
.shade.plain {
  background:
    radial-gradient(ellipse 70% 60% at 78% 100%, rgb(150 40 34 / 0.28), transparent 70%),
    linear-gradient(180deg, #0c0809 0%, #120b0c 100%);
}
.bottom {
  position: absolute;
  inset: auto 0 0 0;
  height: 140px;
  background: linear-gradient(0deg, rgb(0 0 0 / 0.7), transparent);
  opacity: 0;
  transition: opacity 0.2s;
}
.bottom.on {
  opacity: 1;
}
.grain {
  position: absolute;
  inset: 0;
  opacity: 0.07;
  mix-blend-mode: overlay;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.9' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 1 0'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E");
}
.shade.over-art {
  background:
    linear-gradient(90deg, #0c0809 0%, rgb(12 8 9 / 0.75) 22%, rgb(12 8 9 / 0) 55%),
    linear-gradient(180deg, rgb(12 8 9 / 0.85) 0%, rgb(12 8 9 / 0) 30%, rgb(12 8 9 / 0) 60%, #0c0809 100%),
    radial-gradient(ellipse at 60% 45%, transparent 55%, rgb(12 8 9 / 0.7) 100%);
}
</style>
