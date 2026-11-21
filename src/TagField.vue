<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { Plus, X } from 'lucide-vue-next'

const props = defineProps<{ label: string; values: string[] }>()
const emit = defineEmits<{ change: [values: string[]] }>()
const id = useId()
const input = ref<HTMLInputElement>()
const query = ref('')
const expanded = ref(false)
const candidates = computed(() => [...new Set(query.value.split(/[,，]/).map(value => value.trim()).filter(Boolean))])
const showOptions = computed(() => expanded.value && candidates.value.length > 0)

function add() {
  const next = [...new Set([...props.values, ...candidates.value])]
  if (next.length !== props.values.length) emit('change', next)
  query.value = ''
  expanded.value = false
  input.value?.focus()
}
function remove(index: number) {
  emit('change', props.values.filter((_, current) => current !== index))
  input.value?.focus()
}
function keydown(event: KeyboardEvent) {
  if (event.isComposing || event.keyCode === 229) return
  if (event.key === 'Enter' && showOptions.value) {
    event.preventDefault()
    add()
  } else if (event.key === 'ArrowDown' && candidates.value.length) {
    event.preventDefault()
    expanded.value = true
  } else if (event.key === 'Escape' && showOptions.value) {
    event.preventDefault()
    event.stopPropagation()
    expanded.value = false
  } else if (event.key === 'Backspace' && !query.value && props.values.length) {
    event.preventDefault()
    remove(props.values.length - 1)
  }
}
</script>

<template>
  <div class="tag-field">
    <label :for="id">{{ label }}</label>
    <div class="tag-input">
      <span v-for="(value, index) in values" :key="index" class="tag-chip">
        <span :title="value">{{ value }}</span>
        <button type="button" :aria-label="'移除' + label + ' ' + value" @click="remove(index)"><X :size="12" /></button>
      </span>
      <input :id="id" ref="input" v-model="query" role="combobox" type="text" autocomplete="off"
        :aria-expanded="showOptions" :aria-controls="id + '-options'" aria-autocomplete="list"
        :aria-activedescendant="showOptions ? id + '-add' : undefined"
        @focus="expanded = true" @input="expanded = true" @blur="expanded = false" @keydown="keydown" />
    </div>
    <ul v-show="showOptions" :id="id + '-options'" role="listbox" :aria-label="'添加' + label" class="tag-options">
      <li :id="id + '-add'" role="option" aria-selected="true" @mousedown.prevent @click="add">
        <Plus :size="14" /><span>添加「{{ candidates.join('、') }}」</span>
      </li>
    </ul>
  </div>
</template>
