<script setup lang="ts">
import { cva } from 'class-variance-authority'
import { Primitive } from 'reka-ui'

const variants = cva('button', {
  variants: { variant: { default: 'button-primary', outline: 'button-outline', ghost: 'button-ghost' } },
  defaultVariants: { variant: 'default' },
})
withDefaults(defineProps<{ variant?: 'default' | 'outline' | 'ghost'; disabled?: boolean }>(), { variant: 'default' })
</script>

<template>
  <Primitive as="button" type="button" :disabled="disabled" :class="variants({ variant })"><slot /></Primitive>
</template>
