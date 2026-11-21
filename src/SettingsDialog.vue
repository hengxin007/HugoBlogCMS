<script setup lang="ts">
import { computed, ref, watch, watchEffect } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { ArrowUpRight, Check, CircleAlert, FolderOpen, Github, Image, LoaderCircle, Monitor, Moon, Palette, ScanSearch, Settings, Sun } from 'lucide-vue-next'
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import Button from './components/ui/Button.vue'
import Dialog from './components/ui/Dialog.vue'
import { defaultAccent, defaults, type HugoInfo, type Preferences, type ThemeMode } from './preferences'

const version = ref('')
const dataDirectory = ref('')
const props = defineProps<{ preferences: Preferences; hostedAvailable: boolean; previewPort: string }>()
const emit = defineEmits<{ saved: [preferences: Preferences] }>()
const open = defineModel<boolean>('open', { required: true })
const tab = ref('general')
const panelViewport = ref<HTMLElement>()
const panelHeight = ref<number>()
watchEffect(onCleanup => {
  if (!open.value) { panelHeight.value = undefined; return }
  const selector = '[data-settings-panel="' + tab.value + '"]'
  const panel = panelViewport.value?.querySelector<HTMLElement>(selector)
  if (!panel) return
  const measure = () => { panelHeight.value = panel.offsetHeight }
  measure()
  const observer = new ResizeObserver(measure)
  observer.observe(panel)
  onCleanup(() => observer.disconnect())
}, { flush: 'post' })
const form = ref<Preferences>({ ...defaults })
const portInput = ref('')
const saving = ref(false)
const task = ref('')
const error = ref('')
const hugoError = ref('')
const hugoInfo = ref<HugoInfo>()
const dialogOpen = computed({ get: () => open.value, set: value => { if (!saving.value) open.value = value } })
const tabs = [
  { value: 'general', label: '通用' }, { value: 'editor', label: '编辑器' },
  { value: 'appearance', label: '外观' }, { value: 'about', label: '关于' },
]
const themes = [
  { value: 'system', label: '跟随系统', icon: Monitor }, { value: 'light', label: '浅色', icon: Sun },
  { value: 'dark', label: '深色', icon: Moon }, { value: 'custom', label: '自定义', icon: Palette },
] as const
const presets = [defaultAccent, '#335fce', '#7253aa', '#b05238', '#aa3d78']
const donationAssets = import.meta.glob<string>('./assets/donation-*.png', { eager: true, import: 'default', query: '?url' })
const donationSlots = [1, 2].map(index => ({ index, src: donationAssets['./assets/donation-' + index + '.png'] }))
const versionLabel = computed(() => hugoInfo.value?.version.split(/\s+/)[1]?.split('+')[0])

watch(open, value => {
  if (!value) return
  tab.value = 'general'
  form.value = { ...props.preferences, hostedImages: props.preferences.hostedImages ?? false }
  void invoke<string>('data_directory').then(path => { dataDirectory.value = path }).catch(() => { dataDirectory.value = '' })
  portInput.value = props.preferences.previewPort?.toString() ?? ''
  error.value = ''
  hugoError.value = ''
  hugoInfo.value = undefined
})
watch(() => form.value.hugoPath, () => { hugoError.value = ''; hugoInfo.value = undefined }, { flush: 'sync' })

async function checkHugo(detect = false) {
  if (task.value) return
  task.value = detect ? 'detect' : 'verify'
  hugoError.value = ''
  hugoInfo.value = undefined
  try {
    const info = detect ? await invoke<HugoInfo>('detect_hugo') : await invoke<HugoInfo>('inspect_hugo', { path: form.value.hugoPath })
    if (detect) form.value.hugoPath = info.path
    hugoInfo.value = info
  } catch (reason) { hugoError.value = String(reason) }
  finally { task.value = '' }
}

async function browse() {
  task.value = 'browse'
  hugoError.value = ''
  try {
    const path = await invoke<string | null>('pick_hugo')
    if (path) form.value.hugoPath = path
  } catch (reason) { hugoError.value = String(reason) }
  finally { task.value = '' }
  if (form.value.hugoPath && !hugoError.value) await checkHugo()
}

async function save() {
  const value = portInput.value.trim()
  const port = value ? Number(value) : null
  if (port !== null && (!/^\d{4,5}$/.test(value) || port < 1024 || port > 65535)) {
    tab.value = 'general'
    error.value = '请输入 1024–65535 之间的端口，或留空自动分配。'
    return
  }
  saving.value = true
  error.value = ''
  try {
    const next = await invoke<Preferences>('settings', { changes: { ...form.value, previewPort: port } })
    emit('saved', next)
    open.value = false
  } catch (reason) { error.value = String(reason) }
  finally { saving.value = false }
}

watch(dialogOpen, visible => {
  if (visible && !version.value) void getVersion().then(value => { version.value = value }).catch(() => {})
})

async function openRepository() {
  try { await invoke('open_page', { page: 'about_github' }) }
  catch (reason) { error.value = String(reason) }
}
</script>

<template>
  <Dialog v-model:open="dialogOpen" title="设置" description="配置 Hugo 和写作界面。" content-class="settings-dialog">
    <template #icon><Settings :size="19" /></template>
    <TabsRoot v-model="tab" class="settings-tabs">
      <TabsList class="settings-tab-list" :style="{ '--tab-index': tabs.findIndex(item => item.value === tab) }" aria-label="设置分类">
        <TabsTrigger v-for="item in tabs" :key="item.value" :value="item.value" class="settings-tab">{{ item.label }}</TabsTrigger>
      </TabsList>
      <div ref="panelViewport" class="settings-panel-viewport" :style="{ height: panelHeight === undefined ? undefined : panelHeight + 'px' }">
        <TabsContent class="settings-panel" value="general" data-settings-panel="general">
          <section class="settings-section">
            <h3>Hugo 配置</h3>
            <label class="dialog-field" for="settings-hugo">Hugo 路径</label>
            <div class="settings-input-row">
              <input id="settings-hugo" v-model="form.hugoPath" type="text" placeholder="留空使用随应用提供的 Hugo" :disabled="!!task || saving" />
              <Button variant="outline" :disabled="!!task || saving" @click="browse"><FolderOpen :size="15" /> 浏览</Button>
            </div>
            <div class="settings-inline-actions">
              <Button variant="outline" :disabled="!!task || saving" @click="checkHugo()"><Check :size="15" /> 验证</Button>
              <Button variant="outline" :disabled="!!task || saving" @click="checkHugo(true)"><ScanSearch :size="15" /> 自动检测</Button>
              <Button v-if="form.hugoPath" variant="ghost" :disabled="!!task || saving" @click="form.hugoPath = ''">使用内置 Hugo</Button>
              <LoaderCircle v-if="task" :size="16" class="spinner" aria-label="处理中" />
            </div>
            <p v-if="hugoError" class="settings-feedback danger" role="alert"><CircleAlert :size="16" />{{ hugoError }}</p>
            <div v-else-if="hugoInfo" class="settings-feedback success" role="status">
              <Check :size="17" /><div><strong>Hugo {{ versionLabel }}{{ hugoInfo.extended ? ' Extended' : '' }}</strong><small :title="hugoInfo.path">{{ hugoInfo.path }}</small></div>
            </div>
          </section>
          <section class="settings-section">
            <h3>本地预览</h3>
            <label class="dialog-field settings-short" for="settings-port">预览端口</label>
            <div class="settings-input-row settings-port-row">
              <input id="settings-port" v-model="portInput" type="text" inputmode="numeric" maxlength="5" placeholder="自动分配" />
              <span v-if="previewPort" class="settings-tag">当前 {{ previewPort }}</span>
            </div>
            <p class="settings-hint">留空自动分配，固定端口可填写 1024–65535。</p>
          </section>
          <section class="settings-section">
            <h3>网络与图片</h3>
            <label class="dialog-field" for="settings-proxy">HTTP 代理</label>
            <div class="settings-input-row"><input id="settings-proxy" v-model="form.proxy" type="text" placeholder="留空使用本机默认网络" /></div>
            <label v-if="hostedAvailable" class="settings-choice"><input v-model="form.hostedImages" type="checkbox" />发布时使用 GitHub 图床</label>
          </section>
          <section v-if="dataDirectory" class="settings-section">
            <h3>数据与备份</h3>
            <input class="settings-data-directory" aria-label="数据目录" :value="dataDirectory" readonly />
            <p class="settings-hint">关闭 CMS 后，可复制或替换整个 data 目录。</p>
          </section>
        </TabsContent>
        <TabsContent class="settings-panel" value="editor" data-settings-panel="editor">
          <section class="settings-section">
            <h3>Markdown 编辑器</h3>
            <label class="settings-size-label" for="settings-editor-size">正文编辑字号 <output>{{ form.editorFontSize }} px</output></label>
            <input id="settings-editor-size" v-model.number="form.editorFontSize" class="settings-range" type="range" min="10" max="30" />
            <div class="settings-sample settings-code-sample" :style="{ fontSize: form.editorFontSize + 'px' }"><span>1</span> ## 今天写点什么？<br /><span>2</span> 从一个小标题开始。</div>
          </section>
        </TabsContent>
        <TabsContent class="settings-panel" value="appearance" data-settings-panel="appearance">
          <section class="settings-section">
            <h3>界面主题</h3>
            <div class="settings-theme-grid" role="group" aria-label="界面主题">
              <button v-for="theme in themes" :key="theme.value" type="button" class="settings-theme"
                :aria-pressed="form.theme === theme.value" @click="form.theme = theme.value as ThemeMode">
                <component :is="theme.icon" :size="21" /><span>{{ theme.label }}</span>
              </button>
            </div>
            <div v-if="form.theme === 'custom'" class="settings-custom">
              <label for="settings-base">背景模式</label>
              <select id="settings-base" v-model="form.customBase"><option value="system">跟随系统</option><option value="light">浅色</option><option value="dark">深色</option></select>
              <label for="settings-accent">强调色</label>
              <div class="settings-swatches">
                <button v-for="color in presets" :key="color" class="settings-swatch" type="button" :style="{ backgroundColor: color }" :aria-label="color" :aria-pressed="form.accentColor === color" @click="form.accentColor = color" />
                <input id="settings-accent" v-model="form.accentColor" type="color" />
              </div>
            </div>
          </section>
          <section class="settings-section">
            <h3>界面字号</h3>
            <label class="settings-size-label" for="settings-ui-size">菜单和界面文字 <output>{{ form.interfaceFontSize }} px</output></label>
            <input id="settings-ui-size" v-model.number="form.interfaceFontSize" class="settings-range" type="range" min="12" max="20" />
            <div class="settings-sample" :style="{ fontSize: form.interfaceFontSize + 'px' }">文章列表 · 新建文章 · 发布</div>
          </section>
          <section class="settings-section">
            <h3>性能与动效</h3>
            <label class="settings-toggle">
              <span><strong>禁用 GPU 加速</strong><small>保存后重启博客 CMS 生效。</small></span>
              <input v-model="form.disableGpu" type="checkbox" role="switch" />
            </label>
            <label class="settings-toggle">
              <span><strong>禁用动画</strong><small>保存后立即生效。</small></span>
              <input v-model="form.disableAnimations" type="checkbox" role="switch" />
            </label>
          </section>
        </TabsContent>
        <TabsContent class="settings-panel" value="about" data-settings-panel="about">
          <section v-if="version" class="settings-section">
            <h3>博客 CMS</h3>
            <p>版本 {{ version }}</p>
          </section>
          <section class="settings-section">
            <h3>作者 GitHub</h3>
            <button class="settings-repository" type="button" @click="openRepository">
              <Github :size="23" /><span><strong>hengxin007</strong><small>github.com/hengxin007</small></span><ArrowUpRight :size="17" />
            </button>
          </section>
          <section class="settings-section">
            <h3>捐赠支持</h3>
            <div class="settings-donation-grid">
              <figure v-for="slot in donationSlots" :key="slot.index" class="settings-donation">
                <img v-if="slot.src" :src="slot.src" :alt="'捐赠截图 ' + slot.index" />
                <template v-else><Image :size="23" /><figcaption>捐赠截图 {{ slot.index }}</figcaption></template>
              </figure>
            </div>
          </section>
        </TabsContent>
      </div>
    </TabsRoot>
    <p v-if="error" class="settings-feedback danger" role="alert"><CircleAlert :size="16" />{{ error }}</p>
    <div class="dialog-actions settings-actions">
      <Button variant="outline" :disabled="saving || !!task" @click="open = false">取消</Button>
      <Button :disabled="saving || !!task" @click="save"><LoaderCircle v-if="saving" :size="15" class="spinner" /> 保存设置</Button>
    </div>
  </Dialog>
</template>
