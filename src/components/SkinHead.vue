<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'

const props = defineProps<{ dataUrl: string | null; size?: number }>()
const canvas = ref<HTMLCanvasElement | null>(null)
let generation = 0

function draw() {
  const current = ++generation
  const c = canvas.value
  const ctx = c?.getContext('2d')
  if (!c || !ctx) return
  ctx.clearRect(0, 0, 8, 8)
  if (!props.dataUrl) return
  const img = new Image()
  img.onload = () => {
    if (current !== generation) return
    ctx.clearRect(0, 0, 8, 8)
    ctx.imageSmoothingEnabled = false
    ctx.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8)
    ctx.drawImage(img, 40, 8, 8, 8, 0, 0, 8, 8)
  }
  img.src = props.dataUrl
}

onMounted(draw)
watch(() => props.dataUrl, draw)
</script>

<template>
  <canvas
    ref="canvas"
    width="8"
    height="8"
    :style="{ width: `${size ?? 48}px`, height: `${size ?? 48}px`, imageRendering: 'pixelated' }"
  />
</template>
