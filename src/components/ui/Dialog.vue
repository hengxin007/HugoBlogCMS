<script setup lang="ts">
import { DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle } from 'reka-ui'
import { X } from 'lucide-vue-next'

defineProps<{ title: string; description: string; contentClass?: string }>()
const open = defineModel<boolean>('open', { required: true })
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="dialog-overlay" />
      <DialogContent class="dialog-content" :class="contentClass">
        <DialogTitle class="dialog-title"><slot name="icon" />{{ title }}</DialogTitle>
        <DialogDescription class="muted">{{ description }}</DialogDescription>
        <DialogClose class="dialog-close" aria-label="关闭"><X :size="18" /></DialogClose>
        <slot />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
