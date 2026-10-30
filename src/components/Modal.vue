<script lang="ts">
const openModals: symbol[] = []
</script>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'

const props = defineProps<{ open: boolean; title: string; closeLabel?: string }>()
const emit = defineEmits<{ close: [] }>()
const titleId = useId()
const dialog = ref<HTMLElement | null>(null)
const self = Symbol('modal')
let returnFocus: HTMLElement | null = null

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

function focusables(): HTMLElement[] {
  return dialog.value ? Array.from(dialog.value.querySelectorAll<HTMLElement>(FOCUSABLE)) : []
}

function onKey(e: KeyboardEvent) {
  if (openModals[openModals.length - 1] !== self) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  if (e.key !== 'Tab') return
  const items = focusables()
  const first = items[0]
  const last = items[items.length - 1]
  if (!first || !last) {
    e.preventDefault()
    return
  }
  const active = document.activeElement
  const outside = !dialog.value?.contains(active) || active === dialog.value
  if (e.shiftKey && (active === first || outside)) {
    e.preventDefault()
    last.focus()
  } else if (!e.shiftKey && (active === last || outside)) {
    e.preventDefault()
    first.focus()
  }
}

function activate() {
  if (openModals.includes(self)) return
  openModals.push(self)
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
  window.addEventListener('keydown', onKey)
  void nextTick(() => dialog.value?.focus())
}

function deactivate() {
  const i = openModals.indexOf(self)
  if (i === -1) return
  openModals.splice(i, 1)
  window.removeEventListener('keydown', onKey)
  returnFocus?.focus()
  returnFocus = null
}

watch(
  () => props.open,
  (open) => (open ? activate() : deactivate()),
  { immediate: true },
)
onBeforeUnmount(deactivate)
</script>

<template>
  <Teleport to="body">
    <Transition name="modal" :duration="180">
      <div v-if="open" class="fixed inset-0 z-50 grid place-items-center p-8">
        <div class="overlay absolute inset-0 bg-black/45 backdrop-blur-sm" @click="$emit('close')" />
        <div ref="dialog" class="dialog glass-strong relative w-fit max-w-md min-w-72 rounded-2xl p-6 outline-none" role="dialog" aria-modal="true" :aria-labelledby="titleId" tabindex="-1">
          <h2 :id="titleId" class="mb-3 text-lg leading-snug font-semibold text-balance">{{ title }}</h2>
          <slot />
          <div class="mt-6 flex justify-end gap-2">
            <slot name="actions" />
            <button class="btn-ghost" @click="$emit('close')">{{ closeLabel ?? $t('common.close') }}</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>

.modal-enter-active .overlay,
.modal-leave-active .overlay {
  transition: opacity 0.18s ease;
}
.modal-enter-active .dialog,
.modal-leave-active .dialog {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.modal-enter-from .overlay,
.modal-leave-to .overlay,
.modal-enter-from .dialog,
.modal-leave-to .dialog {
  opacity: 0;
}
.modal-enter-from .dialog {
  transform: translateY(8px) scale(0.98);
}
</style>
