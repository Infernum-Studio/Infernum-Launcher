<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'

const win = getCurrentWindow()
</script>

<template>
  <div class="flex items-center">
    <button class="ctrl" :title="$t('titlebar.minimize')" :aria-label="$t('titlebar.minimize')" @click="win.minimize()">
      <svg viewBox="0 0 10 10"><path d="M1.5 5h7" /></svg>
    </button>
    <button class="ctrl" :title="$t('titlebar.maximize')" :aria-label="$t('titlebar.maximize')" @click="win.toggleMaximize()">
      <svg viewBox="0 0 10 10"><rect x="1.5" y="1.5" width="7" height="7" rx="1" /></svg>
    </button>
    <button class="ctrl close" :title="$t('titlebar.close')" :aria-label="$t('titlebar.close')" @click="win.close()">
      <svg viewBox="0 0 10 10"><path d="M2 2l6 6M8 2L2 8" /></svg>
    </button>
  </div>
</template>

<style scoped>
.ctrl {
  display: grid;
  place-items: center;
  width: 38px;
  height: 30px;
  border-radius: 8px;
  color: rgb(246 238 235 / 0.55);
  transition: background 0.12s, color 0.12s;
}
.ctrl svg {
  width: 10px;
  height: 10px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.3;
  stroke-linecap: round;
}
.ctrl:hover {
  color: #fff;
  background: rgb(255 255 255 / 0.08);
}
.close:hover {
  background: var(--color-accent);
}
</style>
