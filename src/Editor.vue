<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { basicSetup } from 'codemirror'
import { markdown } from '@codemirror/lang-markdown'
import { EditorView, keymap } from '@codemirror/view'
import { Annotation, Compartment } from '@codemirror/state'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { formatMarkdown, type Format } from './markdown'

const props = defineProps<{ authoring: string; dark: boolean }>()
const emit = defineEmits<{ change: [value: string]; image: []; save: [] }>()
const element = ref<HTMLElement>()
const fromVue = Annotation.define<boolean>()
let view: EditorView | undefined
const colorScheme = new Compartment()
const darkExtensions = [
  EditorView.theme({ '.cm-cursor, .cm-dropCursor': { borderLeftColor: '#f1f1f1' } }, { dark: true }),
  syntaxHighlighting(HighlightStyle.define([
    { tag: tags.heading, color: '#ededed', fontWeight: 'bold' },
    { tag: tags.strong, fontWeight: 'bold' },
    { tag: tags.emphasis, fontStyle: 'italic' },
    { tag: [tags.link, tags.url], color: '#a8c5ff', textDecoration: 'underline' },
    { tag: tags.quote, color: '#bbbbbb' },
    { tag: tags.monospace, color: '#efc28c' },
    { tag: [tags.processingInstruction, tags.comment], color: '#afafaf' },
    { tag: [tags.keyword, tags.atom], color: '#dab5fa' },
    { tag: [tags.string, tags.number], color: '#efc28c' },
    { tag: tags.invalid, color: '#ffaba1' },
  ])),
]

onMounted(() => {
  view = new EditorView({
    parent: element.value,
    doc: props.authoring,
    extensions: [basicSetup, markdown(), EditorView.lineWrapping, colorScheme.of(props.dark ? darkExtensions : []),
      keymap.of([{ key: 'Ctrl-s', run: () => { emit('save'); return true } }]),
      EditorView.contentAttributes.of({ 'aria-label': 'Markdown 正文', 'aria-keyshortcuts': 'Control+S' }),
      EditorView.updateListener.of(update => {
        if (update.docChanged && !update.transactions.some(transaction => transaction.annotation(fromVue))) {
          emit('change', update.state.doc.toString())
        }
      }),
      EditorView.domEventHandlers({
        paste(event) {
          if (!Array.from(event.clipboardData?.items ?? []).some(item => item.type.startsWith('image/'))) return false
          event.preventDefault()
          emit('image')
          return true
        },
      }),
    ],
  })
})
watch(() => props.authoring, text => {
  if (!view || view.state.doc.toString() === text) return
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, annotations: fromVue.of(true) })
})
watch(() => props.dark, dark => {
  view?.dispatch({ effects: colorScheme.reconfigure(dark ? darkExtensions : []) })
})
onBeforeUnmount(() => view?.destroy())

function insert(text: string) {
  if (!view) return
  const { from, to } = view.state.selection.main
  view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, scrollIntoView: true })
  view.focus()
}

function format(command: Format) {
  if (!view) return
  const { from, to } = view.state.selection.main
  const edit = formatMarkdown(view.state.doc.toString(), from, to, command)
  view.dispatch({
    changes: { from: edit.from, to: edit.to, insert: edit.insert },
    selection: { anchor: edit.selectFrom, head: edit.selectTo },
    scrollIntoView: true,
  })
  view.focus()
}

function focus() { view?.focus() }

defineExpose({ insert, format, focus })
</script>
<template><div ref="element" class="editor-mount" /></template>
