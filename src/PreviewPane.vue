<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { AppWindow, Eye, LoaderCircle, RefreshCw } from 'lucide-vue-next'
import { ContextMenuContent, ContextMenuItem, ContextMenuPortal, ContextMenuRoot, ContextMenuTrigger } from 'reka-ui'
import Button from './components/ui/Button.vue'

const props = defineProps<{ url: string; revision: number; loading: boolean; busy: boolean; error: string }>()
const emit = defineEmits<{ refresh: []; openBrowser: [url: string] }>()
const frame = ref<HTMLIFrameElement>()
const content = ref<HTMLElement>()
const frameSource = ref('')
const currentPage = ref('')

watch(() => props.url, url => {
  frameSource.value = url
  currentPage.value = url
}, { immediate: true })
watch(() => props.revision, () => { frameSource.value = currentPage.value })

function receiveFrameMessage(event: MessageEvent) {
  const iframe = frame.value
  if (!iframe || event.source !== iframe.contentWindow || !props.url) return
  if (event.origin !== new URL(props.url).origin) return
  const message = event.data
  if (message?.channel !== 'hugo-cms-preview' || typeof message.url !== 'string') return
  const page = URL.parse(message.url)
  if (!page || page.origin !== event.origin || page.username || page.password) return
  currentPage.value = page.href

  if (message.type !== 'contextmenu' || !content.value) return
  const { x, y } = message
  if (!Number.isFinite(x) || !Number.isFinite(y)) return
  const rect = iframe.getBoundingClientRect()
  content.value.dispatchEvent(new MouseEvent('contextmenu', {
    bubbles: true,
    cancelable: true,
    clientX: rect.left + Math.max(0, Math.min(x, rect.width)),
    clientY: rect.top + Math.max(0, Math.min(y, rect.height)),
    button: 2,
  }))
}
onMounted(() => window.addEventListener('message', receiveFrameMessage))
onBeforeUnmount(() => window.removeEventListener('message', receiveFrameMessage))
</script>

<template>
  <div class="pane-bar">
    <span><Eye :size="16" /> Hugo 预览</span>
    <div class="preview-controls">
      <LoaderCircle v-if="loading" class="spinner" :size="15" />
      <Button variant="ghost" title="用默认浏览器打开" aria-label="用默认浏览器打开" :disabled="busy || !currentPage" @click="emit('openBrowser', currentPage)"><AppWindow :size="16" /></Button>
      <Button variant="ghost" title="刷新页面" aria-label="刷新页面" :disabled="busy" @click="emit('refresh')"><RefreshCw :size="15" /></Button>
    </div>
  </div>
  <ContextMenuRoot>
    <ContextMenuTrigger as-child>
      <div ref="content" class="preview-content">
        <div v-if="error" class="preview-error"><strong>Hugo 预览失败</strong><pre>{{ error }}</pre></div>
        <iframe v-if="frameSource" ref="frame" :key="revision" :src="frameSource" title="Hugo 主题预览" sandbox="allow-scripts allow-same-origin" referrerpolicy="no-referrer" />
        <div v-else-if="!error" class="preview-empty"><Eye :size="24" /><span>{{ loading ? '正在生成预览…' : '预览将在这里显示' }}</span></div>
      </div>
    </ContextMenuTrigger>
    <ContextMenuPortal>
      <ContextMenuContent class="menu-content">
        <ContextMenuItem class="menu-item" :disabled="busy" @select="emit('refresh')"><RefreshCw :size="15" />刷新页面</ContextMenuItem>
        <ContextMenuItem class="menu-item" :disabled="busy || !currentPage" @select="emit('openBrowser', currentPage)"><AppWindow :size="15" />用默认浏览器打开</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenuPortal>
  </ContextMenuRoot>
</template>
