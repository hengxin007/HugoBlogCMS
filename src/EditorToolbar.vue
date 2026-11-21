<script setup lang="ts">
import { Bold, Code2, FileImage, Heading, Image, Italic, Link2, List, ListOrdered, MoreHorizontal, Quote, Table2, Clipboard, SquareCode } from 'lucide-vue-next'
import { DropdownMenuRoot, DropdownMenuTrigger, DropdownMenuPortal, DropdownMenuContent, DropdownMenuItem } from 'reka-ui'
import type { Format } from './markdown'

defineProps<{ disabled: boolean }>()
const emit = defineEmits<{ format: [command: Format]; image: [source: 'clipboard' | 'file' | 'url'] }>()
const headings = [{ command: 'h1', label: '一级标题' }, { command: 'h2', label: '二级标题' }, { command: 'h3', label: '三级标题' }] as const
const direct = [
  { command: 'list', label: '无序列表', icon: List },
  { command: 'code', label: '代码块', icon: Code2 },
  { command: 'quote', label: '引用', icon: Quote },
  { command: 'link', label: '链接', icon: Link2 },
  { command: 'table', label: '表格', icon: Table2 },
] as const
const extra = [
  { command: 'ordered', label: '有序列表', icon: ListOrdered },
  { command: 'inline-code', label: '行内代码', icon: SquareCode },
] as const
</script>

<template>
  <div class="editor-toolbar" role="group" aria-label="Markdown 编辑工具">
    <span class="toolbar-name">Markdown</span>
    <button class="tool-button" :disabled="disabled" title="粗体" aria-label="粗体" @mousedown.prevent @click="emit('format', 'bold')"><Bold :size="16" /></button>
    <button class="tool-button" :disabled="disabled" title="斜体" aria-label="斜体" @mousedown.prevent @click="emit('format', 'italic')"><Italic :size="16" /></button>
    <DropdownMenuRoot>
      <DropdownMenuTrigger class="tool-button" :disabled="disabled" title="标题" aria-label="标题"><Heading :size="16" /></DropdownMenuTrigger>
      <DropdownMenuPortal><DropdownMenuContent class="menu-content" align="start" :side-offset="6">
        <DropdownMenuItem v-for="heading in headings" :key="heading.command" class="menu-item" @select="emit('format', heading.command)">{{ heading.label }}</DropdownMenuItem>
      </DropdownMenuContent></DropdownMenuPortal>
    </DropdownMenuRoot>
    <span class="toolbar-divider" aria-hidden="true" />
    <button v-for="tool in direct" :key="tool.command" class="tool-button toolbar-wide" :disabled="disabled" :title="tool.label" :aria-label="tool.label" @mousedown.prevent @click="emit('format', tool.command)"><component :is="tool.icon" :size="16" /></button>
    <DropdownMenuRoot>
      <DropdownMenuTrigger class="tool-button" :disabled="disabled" title="插入图片" aria-label="插入图片"><Image :size="16" /></DropdownMenuTrigger>
      <DropdownMenuPortal><DropdownMenuContent class="menu-content" align="start" :side-offset="6">
        <DropdownMenuItem class="menu-item" @select="emit('image', 'clipboard')"><Clipboard :size="15" />从剪贴板粘贴 <kbd>Ctrl+V</kbd></DropdownMenuItem>
        <DropdownMenuItem class="menu-item" @select="emit('image', 'file')"><FileImage :size="15" />选择本地文件</DropdownMenuItem>
        <DropdownMenuItem class="menu-item" @select="emit('image', 'url')"><Link2 :size="15" />插入图片网址</DropdownMenuItem>
      </DropdownMenuContent></DropdownMenuPortal>
    </DropdownMenuRoot>
    <DropdownMenuRoot>
      <DropdownMenuTrigger class="tool-button" :disabled="disabled" title="更多格式" aria-label="更多格式"><MoreHorizontal :size="17" /></DropdownMenuTrigger>
      <DropdownMenuPortal><DropdownMenuContent class="menu-content" align="end" :side-offset="6">
        <DropdownMenuItem v-for="tool in [...direct, ...extra]" :key="tool.command" class="menu-item" @select="emit('format', tool.command)"><component :is="tool.icon" :size="15" />{{ tool.label }}</DropdownMenuItem>
      </DropdownMenuContent></DropdownMenuPortal>
    </DropdownMenuRoot>
  </div>
</template>
