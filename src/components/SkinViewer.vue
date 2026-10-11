<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { IdleAnimation, SkinViewer } from 'skinview3d'
import SkinHead from './SkinHead.vue'

const props = defineProps<{ dataUrl: string; slim: boolean; width?: number; height?: number }>()
const canvas = ref<HTMLCanvasElement | null>(null)
const failed = ref(false)
let viewer: SkinViewer | null = null

function load() {
  viewer?.loadSkin(props.dataUrl, { model: props.slim ? 'slim' : 'default' })
}

onMounted(() => {
  if (!canvas.value) return
  try {
    viewer = new SkinViewer({
      canvas: canvas.value,
      width: props.width ?? 260,
      height: props.height ?? 380,
      zoom: 0.85,
      fov: 45,
    })
    viewer.animation = new IdleAnimation()
    viewer.controls.enableZoom = false
    viewer.controls.enablePan = false
    viewer.camera.position.x = -20
    viewer.camera.lookAt(0, 0, 0)
    load()
  } catch {
    failed.value = true
  }
})

watch(() => [props.dataUrl, props.slim], load)
onBeforeUnmount(() => viewer?.dispose())
</script>

<template>
  <SkinHead v-if="failed" :data-url="dataUrl" :size="128" />
  <canvas v-else ref="canvas" class="cursor-grab active:cursor-grabbing" />
</template>
