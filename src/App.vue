<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { Channel, invoke, isTauri } from '@tauri-apps/api/core'
import { ArrowUpRight, BookOpen, Check, CircleAlert, Columns2, Copy, Eye, FileText, Fingerprint, FolderOpen, GitBranch, ImagePlus, Link2, LoaderCircle, PanelLeft, PanelLeftClose, PanelLeftOpen, Pencil, Trash2, Plus, RefreshCw, Search, Send, Settings2 } from 'lucide-vue-next'
import { ContextMenuRoot, ContextMenuTrigger, ContextMenuPortal, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, SplitterGroup, SplitterPanel, SplitterResizeHandle } from 'reka-ui'
import Button from './components/ui/Button.vue'
import Dialog from './components/ui/Dialog.vue'
import Editor from './Editor.vue'
import EditorToolbar from './EditorToolbar.vue'
import PreviewPane from './PreviewPane.vue'
import SettingsDialog from './SettingsDialog.vue'
import TagField from './TagField.vue'
import { accentText, defaults, type Preferences } from './preferences'
import Onboarding from './Onboarding.vue'
import appIcon from '../src-tauri/icons/icon.ico?url'
import type { Site, Startup } from './onboarding'
import './onboarding.css'
import type { Format } from './markdown'
import { copyArticle, releaseMarkdown, defaultSlug, listItems, newArticle, parseArticle, readFields, serializeArticle, updateField } from './article'
import type { Article } from './article'

type Picture = { id: string; path: string; hosted: boolean }
type Draft = { id: string; path: string; originalPath: string | null; originalHash: string | null; markdown: string; images: Picture[] }
type StoredArticle = { path: string; markdown: string; hash: string }
type Library = { articles: StoredArticle[]; drafts: Draft[]; warning?: string }
type RepositoryInfo = { remote: string; modules: { path: string; url: string }[] }
type RepositoryRefresh = { repository: RepositoryInfo; library: Library }
type Summary = { url: string; files: string[]; hostedFiles: string[]; approval: string; diff: string }
type ExternalMarkdown = { markdown: string; baseHash: string }
type Published = { path?: string; commit: string }
type PublishResult = Published & { article: StoredArticle }

type Entry = { key: string; title: string; meta: string; draft?: Draft; saved?: StoredArticle }
const articles = ref<StoredArticle[]>([])
const drafts = ref<Draft[]>([])
const active = ref<Draft>()
const startup = ref<Startup>()
const showWizard = ref(false)
const site = ref<Site>()
const deploymentState = ref('')
const checkingDeployment = ref(false)
const repoName = computed(() => site.value?.remote.replace(/^https:\/\/github\.com\//, '').replace(/\.git$/, ''))
const siteHost = computed(() => site.value ? new URL(site.value.website).host : '')
const postsDir = computed(() => site.value?.postsDir ?? 'content/post')
const articlePath = (slug: string) => postsDir.value + '/' + slug + '/index.md'
const supportsEditing = (source: string) => { try { readFields(parseArticle(source).header); return true } catch { return false } }
const editor = ref<InstanceType<typeof Editor>>()
const query = ref('')
const filter = ref<'all' | 'draft'>('all')
const busy = ref('')
const error = ref('')
const previewError = ref('')
const saveState = ref('')
const previewUrl = ref('')
const previewPort = computed(() => previewUrl.value ? new URL(previewUrl.value).port : '')
const previewRevision = ref(0)
const previewing = ref(false)
const metaOpen = ref(true)
const settingsOpen = ref(false)
const repositoryOpen = ref(false)
const repositoryInfo = ref<RepositoryInfo>()
const repositoryLoading = ref(false)
const repositoryRefreshing = ref(false)
const repositoryError = ref('')
const repositoryMessage = ref('')
const externalConflictId = ref<string>()
const externalConflictOpen = computed({
  get: () => externalConflictId.value !== undefined,
  set: open => { if (!open) externalConflictId.value = undefined },
})
let externalTimer: ReturnType<typeof setInterval> | undefined
let checkingExternal = false
let externalWarning = ''
const preferences = ref<Preferences>({ ...defaults })
const systemTheme = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(systemTheme.matches)
const followSystemTheme = (event: MediaQueryListEvent) => { systemDark.value = event.matches }
const dark = computed(() => {
  const mode = preferences.value.theme === 'custom' ? preferences.value.customBase : preferences.value.theme
  return mode === 'dark' || (mode === 'system' && systemDark.value)
})
type ImageTarget = 'body' | 'cover'
const imageFile = ref<HTMLInputElement>()
const pendingImage = ref<{ id: string; target: ImageTarget }>()
const imageUrlOpen = ref(false)
const imageUrl = ref('')
const imageUrlError = ref('')
const viewMode = ref<'edit' | 'split' | 'preview'>('split')
const viewOptions = [
  { value: 'edit', label: '仅编辑', icon: Pencil },
  { value: 'split', label: '左右分栏', icon: Columns2 },
  { value: 'preview', label: '仅预览', icon: Eye },
] as const
const sidebarOpen = ref(true)
const deleteTarget = ref<StoredArticle>()
const deleteOpen = ref(false)
const discardTarget = ref<Entry>()
const discardOpen = ref(false)
const copiedLink = ref(false)
const publishOpen = ref(false)
const summary = ref<Summary>()
const published = ref<Published>()
const yamlOpen = ref(false)
const yamlBuffer = ref('')
const yamlError = ref('')
const slugInput = ref('')
let saveTimer: ReturnType<typeof setTimeout> | undefined
let previewTimer: ReturnType<typeof setTimeout> | undefined
let saveQueue: Promise<void> = Promise.resolve()
let previewGeneration = 0
let quitRequested = false

function fieldsOf(source: string) {
  try { return readFields(parseArticle(source).header) } catch { return {} }
}
function text(value: unknown) { return typeof value === 'string' || typeof value === 'number' ? String(value) : '' }
function itemTitle(source: string, path: string) { return text(fieldsOf(source).title) || path.split('/').at(-1) || '未命名文章' }
const document = computed(() => {
  try { return active.value ? parseArticle(active.value.markdown) : undefined } catch { return undefined }
})
const fields = computed(() => active.value ? fieldsOf(active.value.markdown) : {})
const words = computed(() => (document.value?.body.match(/[\p{Script=Han}]|[\p{L}\p{N}]+/gu) ?? []).length)
const entries = computed<Entry[]>(() => {
  const local = drafts.value.map(draft => ({ key: draft.id, draft, title: itemTitle(draft.markdown, draft.path), meta: draft.originalPath ? '本地修改' : '本地草稿' }))
  const stagedPaths = new Set(drafts.value.flatMap(draft => draft.originalPath ? [draft.originalPath] : []))
  const publishedArticles = articles.value.filter(article => !stagedPaths.has(article.path)).map(saved => {
    const meta = fieldsOf(saved.markdown)
    const date = text(meta.date).slice(0, 10)
    return { key: saved.path, saved, title: itemTitle(saved.markdown, saved.path), meta: meta.draft === true ? '仓库草稿' : date || '已发布', isDraft: meta.draft === true }
  })
  const visible = filter.value === 'draft' ? [...local, ...publishedArticles.filter(item => item.isDraft)] : [...local, ...publishedArticles]
  const term = query.value.trim().toLocaleLowerCase()
  return term ? visible.filter(item => (item.title + ' ' + item.meta + ' ' + (('draft' in item ? item.draft?.path : item.saved.path) ?? '')).toLocaleLowerCase().includes(term)) : visible
})

function clone(draft: Draft) { return JSON.parse(JSON.stringify(draft)) as Draft }
function applyLibrary(library: Library) {
  articles.value = library.articles
  drafts.value = library.drafts
  if (library.warning) error.value = library.warning
  if (!active.value && library.drafts.length) activate(clone(library.drafts[0]))
}
async function refresh(sync: boolean) {
  busy.value = sync ? '正在同步 GitHub…' : '正在读取文章…'
  error.value = ''
  try {
    applyLibrary(await invoke<Library>('library', { sync }))
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
async function showRepository() {
  repositoryOpen.value = true
  if (repositoryLoading.value || repositoryRefreshing.value) return
  repositoryLoading.value = true
  repositoryError.value = ''
  repositoryMessage.value = ''
  repositoryInfo.value = undefined
  try { repositoryInfo.value = await invoke<RepositoryInfo>('repository_info') }
  catch (reason) { repositoryError.value = String(reason) }
  finally { repositoryLoading.value = false }
}
async function refreshRepository() {
  if (busy.value || repositoryLoading.value) return
  busy.value = '正在刷新仓库和子模块…'
  repositoryRefreshing.value = true
  repositoryError.value = ''
  repositoryMessage.value = ''
  try {
    await flush()
    const result = await invoke<RepositoryRefresh>('refresh_repository')
    repositoryInfo.value = result.repository
    applyLibrary(result.library)
    clearTimeout(previewTimer)
    const generation = ++previewGeneration
    if (active.value) {
      await renderPreview(generation, true)
      if (previewError.value) repositoryError.value = '仓库已刷新，Hugo 预览失败。\n' + previewError.value
      else repositoryMessage.value = '仓库和本地预览已刷新。'
    } else {
      repositoryMessage.value = '仓库已刷新。打开一篇文章即可查看预览。'
    }
  } catch (reason) { repositoryError.value = String(reason) }
  finally { repositoryRefreshing.value = false; busy.value = '' }
}
function activate(draft: Draft) {
  previewGeneration++
  clearTimeout(previewTimer)
  previewUrl.value = ''
  previewError.value = ''
  previewing.value = false
  active.value = draft
  externalWarning = ''
  slugInput.value = text(fieldsOf(draft.markdown).slug) || draft.path.split('/').at(-1)?.replace(/\.md$/, '') || ''
  saveState.value = drafts.value.some(item => item.id === draft.id) ? '已保存草稿' : ''
  schedulePreview(0)
}
function flush() {
  clearTimeout(saveTimer)
  const draft = active.value
  if (!draft || !['保存中…', '保存失败'].includes(saveState.value)) return saveQueue
  const snapshot = clone(draft)
  saveQueue = saveQueue.catch(() => {}).then(async () => {
    await invoke('save_draft', { draft: snapshot })
    const index = drafts.value.findIndex(item => item.id === snapshot.id)
    if (index >= 0) drafts.value[index] = snapshot
    else drafts.value.unshift(snapshot)
    if (active.value?.id === snapshot.id && JSON.stringify(active.value) === JSON.stringify(snapshot)) saveState.value = '已保存草稿'
  }).catch(reason => {
    error.value = String(reason)
    if (active.value?.id === snapshot.id) saveState.value = '保存失败'
    throw reason
  })
  return saveQueue
}
function saveDraft() {
  if (!busy.value) void flush().catch(() => {})
}
function apply(source: string) {
  if (!active.value || source === active.value.markdown) return
  active.value.markdown = source
  markChanged()
}
async function markdownHash(markdown: string) {
  const normalized = markdown.replace(/^\uFEFF+/, '').replace(/\r\n?|\n/g, '\r\n')
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(normalized))
  return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('')
}
async function openExternalMarkdown() {
  const draft = active.value
  if (!draft || busy.value) return
  busy.value = '正在打开外部 Markdown 编辑器…'
  try {
    if (!drafts.value.some(item => item.id === draft.id)) saveState.value = '保存中…'
    await flush()
    await invoke('open_external_markdown', { draft: clone(draft) })
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
async function refreshExternalMarkdown(manual = false, confirmed = false) {
  const draft = active.value
  if (!draft || busy.value || showWizard.value || publishOpen.value || yamlOpen.value || checkingExternal) return
  if (!manual && externalConflictOpen.value) return
  checkingExternal = true
  try {
    const incoming = await invoke<ExternalMarkdown | null>('read_external_markdown', { id: draft.id })
    if (active.value !== draft || busy.value || publishOpen.value || yamlOpen.value || showWizard.value) return
    if (!incoming) { externalWarning = ''; return }
    const parsed = parseArticle(incoming.markdown)
    const updatedFields = readFields(parsed.header)
    if (JSON.stringify(updatedFields.slug) !== JSON.stringify(fields.value.slug) || JSON.stringify(updatedFields.url) !== JSON.stringify(fields.value.url)) {
      throw new Error('外部文件修改了 slug 或 url。请在外部文件中恢复这两个字段，再保存。')
    }
    const local = draft.markdown
    const localHash = await markdownHash(local)
    if (active.value !== draft || draft.markdown !== local || busy.value || publishOpen.value || yamlOpen.value || showWizard.value) return
    if (!confirmed && localHash !== incoming.baseHash) {
      if (manual) externalConflictId.value = draft.id
      throw new Error('CMS 和外部文件都有改动。可在 Markdown 右键菜单选择「刷新外部修改」并确认是否导入。')
    }
    if (error.value === externalWarning) error.value = ''
    externalWarning = ''
    apply(incoming.markdown)
    await flush()
    await invoke('read_external_markdown', { id: draft.id })
    if (active.value === draft) schedulePreview(0, true)
  } catch (reason) {
    const message = String(reason).replace(/^Error: /, '')
    if (active.value === draft && (manual || externalWarning !== message)) {
      externalWarning = message
      error.value = message
    }
  } finally { checkingExternal = false }
}
function confirmExternalMarkdown() {
  if (active.value?.id === externalConflictId.value) void refreshExternalMarkdown(true, true)
  externalConflictOpen.value = false
}
function markChanged() {
  saveState.value = '保存中…'
  clearTimeout(saveTimer)
  saveTimer = setTimeout(() => { void flush().catch(() => {}) }, 250)
  schedulePreview()
}
function schedulePreview(delay = 650, refreshPage = false) {
  clearTimeout(previewTimer)
  const generation = ++previewGeneration
  if (!active.value) return
  previewTimer = setTimeout(() => { void renderPreview(generation, refreshPage) }, delay)
}
async function renderPreview(generation: number, refreshPage: boolean) {
  const draft = active.value
  if (!draft || generation !== previewGeneration) return
  previewing.value = true
  try {
    const url = await invoke<string>('preview_article', { draft: clone(draft) })
    if (generation === previewGeneration) {
      if (refreshPage && previewUrl.value === url) previewRevision.value++
      previewUrl.value = url
      previewError.value = ''
    }
  } catch (reason) { if (generation === previewGeneration) previewError.value = String(reason) }
  finally { if (generation === previewGeneration) previewing.value = false }
}
async function openPreviewBrowser(url: string) {
  try { await invoke('open_preview', { url }) }
  catch (reason) { error.value = String(reason) }
}
function fromStored(saved: StoredArticle): Draft {
  return { id: crypto.randomUUID(), path: saved.path, originalPath: saved.path, originalHash: saved.hash, markdown: saved.markdown, images: [] }
}
function hasPublishedLink(item: Entry) {
  const path = item.draft?.originalPath ?? item.saved?.path
  const original = articles.value.find(article => article.path === path)
  if (!original) return false
  const meta = fieldsOf(original.markdown)
  const publishDate = Date.parse(text(meta.publishDate ?? meta.publishdate ?? meta.date))
  const expiryDate = Date.parse(text(meta.expiryDate ?? meta.expirydate))
  return meta.draft !== true && !(publishDate > Date.now()) && !(expiryDate <= Date.now())
}
async function copyLink(item: Entry) {
  const path = item.draft?.originalPath ?? item.saved?.path
  if (!path) return
  error.value = ''
  copiedLink.value = false
  try { await invoke<string>('copy_article_link', { path }); copiedLink.value = true }
  catch (reason) { error.value = String(reason) }
}
async function openArticleDirectory(item: Entry) {
  const path = item.saved?.path ?? item.draft?.originalPath ?? null
  const draftId = path ? null : item.draft?.id ?? null
  error.value = ''
  try { await invoke('open_article_directory', { path, draftId }) }
  catch (reason) { error.value = String(reason) }
}
async function duplicate(item: Entry) {
  if (busy.value) return
  busy.value = '正在复制为新草稿…'
  error.value = ''
  try {
    await flush()
    const original = item.key === active.value?.id ? active.value : item.draft ?? (item.saved && fromStored(item.saved))
    if (!original) return
    const slug = defaultSlug() + '-' + crypto.randomUUID().slice(0, 6)
    const copy = { id: crypto.randomUUID(), path: articlePath(slug), originalPath: null, originalHash: null, markdown: copyArticle(original.markdown, slug), images: [] }
    const saved = await invoke<Draft>('duplicate_draft', { original: clone(original), copy })
    drafts.value.unshift(saved)
    filter.value = 'all'
    query.value = ''
    viewMode.value = 'split'
    activate(clone(saved))
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
function requestDiscard(item: Entry) {
  discardTarget.value = item
  discardOpen.value = true
}
async function discard() {
  const draft = discardTarget.value?.draft
  if (!draft || busy.value) return
  discardOpen.value = false
  busy.value = '正在移除本地草稿…'
  error.value = ''
  const wasActive = active.value?.id === draft.id
  if (wasActive) { clearTimeout(saveTimer); clearTimeout(previewTimer); previewGeneration++ }
  try {
    if (wasActive) await saveQueue.catch(() => {})
    else await flush()
    await invoke('discard_draft', { id: draft.id })
    drafts.value = drafts.value.filter(item => item.id !== draft.id)
    if (wasActive) {
      const original = articles.value.find(article => article.path === draft.originalPath)
      if (original) {
        filter.value = 'all'
        query.value = ''
        activate(fromStored(original))
      } else {
        active.value = undefined
        previewUrl.value = ''
        previewError.value = ''
        previewing.value = false
        saveState.value = ''
      }
    }
  } catch (reason) {
    error.value = String(reason)
    if (wasActive) markChanged()
  } finally { busy.value = '' }
}
async function select(item: Entry) {
  if (active.value && (active.value.id === item.key || active.value.originalPath === item.key)) return
  try { await flush() } catch { return }
  error.value = ''
  if (item.draft) activate(clone(item.draft))
  if (item.saved) activate(fromStored(item.saved))
}
async function create() {
  try { await flush() } catch { return }
  const slug = defaultSlug()
  if (viewMode.value === 'preview') viewMode.value = 'split'
  filter.value = 'all'
  query.value = ''
  activate({ id: crypto.randomUUID(), path: articlePath(slug), originalPath: null, originalHash: null, markdown: newArticle(slug), images: [] })
  markChanged()
}
function body(value: string) {
  if (document.value) apply(serializeArticle({ ...document.value, body: value }))
  else apply(value)
}
function field(key: string, value: unknown) {
  if (!document.value || fields.value[key] === value) return
  try { apply(serializeArticle(updateField(document.value, key, value))) }
  catch (reason) { error.value = String(reason) }
}
function updateSlug(value: string) {
  if (!active.value || active.value.originalPath || !document.value) return
  const slug = value.trim()
  if (slug === fields.value.slug) return
  if (!/^[a-z0-9](?:[a-z0-9-]{0,88}[a-z0-9])?$/.test(slug) || /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/.test(slug)) {
    error.value = 'Slug 请使用小写字母、数字和连字符，长度不超过 90 个字符。'
    slugInput.value = text(fields.value.slug)
    return
  }
  active.value.path = articlePath(slug)
  field('slug', slug)
}
function format(command: Format) {
  if (!busy.value) editor.value?.format(command)
}
async function attachImage(target: ImageTarget, png?: number[]) {
  const draft = active.value
  if (!draft || busy.value) return
  busy.value = png ? '正在导入图片…' : '正在读取剪贴板…'
  error.value = ''
  try {
    const picture = await invoke<Picture>(png ? 'import_image' : 'insert_image', {
      draft: clone(draft), hosted: !!site.value?.hostedImages && !!preferences.value.hostedImages, ...(png && { png }),
    })
    draft.images.push(picture)
    if (target === 'cover') field('image', picture.path)
    else editor.value?.insert('![图片说明](' + picture.path + ')')
    markChanged()
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
  if (target === 'body') { await nextTick(); editor.value?.focus() }
}
function imageAction(source: 'clipboard' | 'file' | 'url', target: ImageTarget = 'body') {
  if (!active.value || busy.value) return
  if (source === 'clipboard') void attachImage(target)
  else if (source === 'file') {
    pendingImage.value = { id: active.value.id, target }
    imageFile.value?.click()
  } else {
    imageUrl.value = ''
    imageUrlError.value = ''
    imageUrlOpen.value = true
  }
}
async function importFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  const pending = pendingImage.value
  input.value = ''
  pendingImage.value = undefined
  if (!pending || !file || pending.id !== active.value?.id || busy.value) return
  error.value = ''
  busy.value = '正在读取本地图片…'
  let png: number[] | undefined
  try {
    if (file.size > 24 * 1024 * 1024) throw new Error('图片超过 24 MB。')
    const bitmap = await createImageBitmap(file)
    try {
      if (bitmap.width > 8192 || bitmap.height > 8192 || bitmap.width * bitmap.height > 24_000_000) throw new Error('图片尺寸超出支持范围。')
      const canvas = window.document.createElement('canvas')
      canvas.width = bitmap.width
      canvas.height = bitmap.height
      canvas.getContext('2d')!.drawImage(bitmap, 0, 0)
      const blob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, 'image/png'))
      if (!blob || blob.size > 24 * 1024 * 1024) throw new Error('转换后的图片超过 24 MB。')
      png = Array.from(new Uint8Array(await blob.arrayBuffer()))
    } finally { bitmap.close() }
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
  if (png && pending.id === active.value?.id) await attachImage(pending.target, png)
}
function insertImageUrl() {
  try {
    const url = new URL(imageUrl.value.trim())
    if (!['https:', 'http:'].includes(url.protocol)) throw new Error()
    const encoded = url.href.replaceAll('(', '%28').replaceAll(')', '%29')
    editor.value?.insert('![图片说明](' + encoded + ')')
    imageUrlOpen.value = false
  } catch { imageUrlError.value = '请输入有效的 HTTP 或 HTTPS 图片网址。' }
}
function openYaml() {
  yamlBuffer.value = active.value?.markdown ?? ''
  yamlError.value = ''
  yamlOpen.value = true
}
function saveYaml() {
  try {
    const updated = parseArticle(yamlBuffer.value)
    const next = readFields(updated.header)
    if (JSON.stringify(next.slug) !== JSON.stringify(fields.value.slug) || JSON.stringify(next.url) !== JSON.stringify(fields.value.url)) throw new Error('请保留 YAML 中的 slug 和 url。新文章可通过 Slug 表单调整地址。')
    apply(serializeArticle(updated as Article))
    yamlOpen.value = false
  } catch (reason) { yamlError.value = String(reason) }
}
async function prepare() {
  if (!active.value) return
  if (!text(fields.value.title).trim()) { error.value = '请输入文章标题。'; return }
  busy.value = '正在准备发布摘要…'
  error.value = ''
  try {
    await flush()
    summary.value = await invoke<Summary>('prepare_publish', { draft: clone(active.value), releaseMarkdown: releaseMarkdown(active.value.markdown) })
    publishOpen.value = true
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
async function publish() {
  if (!active.value || !summary.value) return
  publishOpen.value = false
  busy.value = '正在构建并推送…'
  error.value = ''
  clearTimeout(saveTimer)
  clearTimeout(previewTimer)
  previewGeneration++
  const draft = clone(active.value)
  try {
    const result = await invoke<PublishResult>('publish_article', { draft, approval: summary.value.approval, releaseMarkdown: releaseMarkdown(draft.markdown) })
    const { article, ...deployment } = result
    published.value = deployment
    drafts.value = drafts.value.filter(item => item.id !== draft.id)
    await refresh(true)
    const stored = articles.value.find(item => item.path === article.path)
    const current = stored && !error.value ? stored : article
    if (current !== stored) articles.value = [...articles.value.filter(item => item.path !== article.path), current]
    activate({ ...fromStored(current), id: draft.id })
    if (!entries.value.some(item => item.key === current.path)) {
      filter.value = 'all'
      if (!entries.value.some(item => item.key === current.path)) query.value = ''
    }
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
function repositoryArticle(item: Entry) {
  return item.saved ?? articles.value.find(article => article.path === item.draft?.originalPath)
}
function requestDelete(item: Entry) {
  deleteTarget.value = repositoryArticle(item)
  deleteOpen.value = !!deleteTarget.value
}
async function removeArticle() {
  const target = deleteTarget.value
  if (!target || busy.value) return
  deleteOpen.value = false
  busy.value = '正在构建并推送删除提交…'
  error.value = ''
  try {
    await flush()
    const commit = await invoke<string>('delete_article', { path: target.path, hash: target.hash })
    if (active.value?.originalPath === target.path) {
      clearTimeout(previewTimer)
      previewGeneration++
      active.value = undefined
      previewUrl.value = ''
      previewError.value = ''
      previewing.value = false
      saveState.value = ''
    }
    published.value = { commit }
    await refresh(true)
    articles.value = articles.value.filter(article => article.path !== target.path)
    if (active.value) schedulePreview(0, true)
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
function applySettings(next: Preferences) {
  const previous = preferences.value
  preferences.value = next
  if (next.hugoPath !== previous.hugoPath || next.previewPort !== previous.previewPort || next.proxy !== previous.proxy) {
    previewUrl.value = ''
    previewError.value = ''
    schedulePreview(0)
  }
}
watch(() => published.value?.commit, commit => { deploymentState.value = ''; if (commit) void refreshDeployment(commit) })
watch([preferences, dark], ([settings, isDark]) => {
  const root = window.document.documentElement
  root.dataset.theme = isDark ? 'dark' : 'light'
  root.dataset.motion = settings.disableAnimations ? 'off' : 'on'
  root.style.setProperty('--ui-font-size', settings.interfaceFontSize + 'px')
  root.style.setProperty('--editor-font-size', settings.editorFontSize + 'px')
  if (settings.theme === 'custom') {
    root.style.setProperty('--custom-accent', settings.accentColor)
    root.style.setProperty('--dark-custom-ink', 'color-mix(in srgb, ' + settings.accentColor + ' 48%, #fff)')
    const onAccent = accentText(settings.accentColor)
    const inkStrength = onAccent === '#000' ? 36 : 70
    root.style.setProperty('--light-custom-ink', `color-mix(in srgb, var(--custom-accent) ${inkStrength}%, #161616)`)
    root.style.setProperty('--on-accent', onAccent)
  } else {
    root.style.removeProperty('--custom-accent')
    root.style.removeProperty('--dark-custom-ink')
    root.style.removeProperty('--light-custom-ink')
    root.style.removeProperty('--on-accent')
  }
}, { immediate: true })
async function loadEditor(next: Site) {
  const returning = !!site.value
  site.value = next
  showWizard.value = false
  try {
    preferences.value = await invoke<Preferences>('settings')
    await refresh(!returning)
    if (returning && active.value) schedulePreview(0)
  } catch (reason) { error.value = String(reason) }
}
async function openSiteWizard() {
  if (busy.value) return
  busy.value = '正在打开站点向导…'
  try {
    await flush()
    startup.value = await invoke<Startup>('startup')
    clearTimeout(previewTimer)
    previewGeneration++
    previewUrl.value = ''
    showWizard.value = true
  } catch (reason) { error.value = String(reason) }
  finally { busy.value = '' }
}
async function refreshDeployment(commit: string, explicit = false) {
  checkingDeployment.value = true
  try {
    const status = await invoke<string | null>('deployment_status', { commit })
    if (published.value?.commit === commit) deploymentState.value = status ?? (explicit ? '请在 Cloudflare 控制台查看部署状态' : '')
  } catch (reason) {
    if (explicit && published.value?.commit === commit) deploymentState.value = String(reason)
  } finally { checkingDeployment.value = false }
}
async function openPage(page: string, path?: string) {
  try { await invoke('open_page', { page, path }) } catch (reason) { error.value = String(reason) }
}
async function quitWhenIdle() {
  if (!quitRequested || busy.value) return
  quitRequested = false
  busy.value = '正在保存草稿并退出…'
  try {
    await flush()
    await invoke('quit_application')
  } catch (reason) {
    quitRequested = false
    busy.value = ''
    error.value = String(reason)
    void invoke('show_main_window').catch(() => {})
  }
}
watch(busy, () => { void quitWhenIdle() })
onMounted(async () => {
  systemTheme.addEventListener('change', followSystemTheme)
  if (!isTauri()) { error.value = '请使用 Windows 客户端打开博客 CMS。'; return }
  externalTimer = setInterval(() => { void refreshExternalMarkdown() }, 1250)
  const quitChannel = new Channel<null>(() => {
    quitRequested = true
    void quitWhenIdle()
  })
  try {
    await invoke('register_tray_quit', { channel: quitChannel })
    preferences.value = await invoke<Preferences>('settings')
    const legacyHostedImages = localStorage.getItem('hosted-images')
    if (legacyHostedImages !== null) {
      if (preferences.value.hostedImages === null) {
        preferences.value = await invoke<Preferences>('settings', { changes: { ...preferences.value, hostedImages: legacyHostedImages === 'true' } })
      }
      localStorage.removeItem('hosted-images')
    }
    startup.value = await invoke<Startup>('startup')
    if (startup.value.site && !startup.value.showWelcome) await loadEditor(startup.value.site)
  } catch (reason) { error.value = String(reason) }
})
onBeforeUnmount(() => { systemTheme.removeEventListener('change', followSystemTheme); clearTimeout(saveTimer); clearTimeout(previewTimer); clearInterval(externalTimer) })
</script>

<template>
  <Onboarding v-if="startup && (!site || showWizard)" :initial="startup" @finished="loadEditor" />
  <div v-else-if="!site" class="setup-shell"><main class="setup-welcome"><p v-if="error" role="alert" class="setup-error">{{ error }}</p><p v-else role="status">正在加载博客 CMS…</p></main></div>
  <div v-else class="shell">
    <header class="appbar">
      <div class="brand"><img class="brand-mark" :src="appIcon" alt="" width="36" height="36" /><div><strong>博客 CMS</strong><span :title="site.title">{{ siteHost }}</span></div></div>
      <div class="app-actions"><span class="branch"><GitBranch :size="14" /> {{ site.branch }}</span><Button variant="ghost" :disabled="!!busy" @click="openSiteWizard">站点向导</Button><Button variant="ghost" title="应用设置" aria-label="应用设置" :disabled="!!busy" @click="settingsOpen = true"><Settings2 :size="18" /></Button></div>
    </header>
    <div class="layout" :class="{ 'sidebar-collapsed': !sidebarOpen }">
      <aside :inert="!sidebarOpen" id="article-sidebar" class="sidebar" aria-label="文章列表">
        <div class="sidebar-head"><div class="sidebar-title"><h1>文章</h1><Button variant="ghost" title="同步 GitHub" aria-label="同步 GitHub" :disabled="!!busy" @click="refresh(true)"><RefreshCw :size="16" /></Button></div>
          <Button class="new-post" :disabled="!!busy" @click="create"><Plus :size="17" /> 新建文章</Button>
          <label class="search"><Search :size="15" /><input v-model="query" placeholder="搜索文章" aria-label="搜索文章" /></label>
          <div class="filters"><button :class="{ selected: filter === 'all' }" @click="filter = 'all'">全部</button><button :class="{ selected: filter === 'draft' }" @click="filter = 'draft'">草稿 <span>{{ drafts.length }}</span></button></div>
        </div>
        <div class="post-list">
          <ContextMenuRoot v-for="item in entries" :key="item.key">
            <ContextMenuTrigger as-child :disabled="!!busy">
              <button class="post-item" :class="{ current: active?.id === item.key || active?.originalPath === item.key }" :disabled="!!busy" @click="select(item)">
                <FileText :size="16" /><span><strong>{{ item.title }}</strong><small>{{ item.meta }}</small></span>
              </button>
            </ContextMenuTrigger>
            <ContextMenuPortal>
              <ContextMenuContent class="menu-content">
                <ContextMenuItem class="menu-item" @select="select(item)"><Pencil :size="15" />继续编辑</ContextMenuItem>
                <ContextMenuItem v-if="supportsEditing(item.draft?.markdown ?? item.saved?.markdown ?? '')" class="menu-item" @select="duplicate(item)"><Copy :size="15" />复制为新草稿</ContextMenuItem>
                <ContextMenuItem v-if="hasPublishedLink(item)" class="menu-item" @select="copyLink(item)"><Link2 :size="15" />复制文章链接</ContextMenuItem>
                <ContextMenuItem class="menu-item" @select="openArticleDirectory(item)"><FolderOpen :size="15" />打开文章目录</ContextMenuItem>
                <template v-if="item.draft || repositoryArticle(item)">
                  <ContextMenuSeparator class="menu-separator" />
                  <ContextMenuItem v-if="item.draft" class="menu-item menu-danger" @select="requestDiscard(item)"><Trash2 :size="15" />{{ item.draft.originalPath ? '放弃本地修改' : '删除草稿' }}</ContextMenuItem>
                  <ContextMenuItem v-if="repositoryArticle(item)" class="menu-item menu-danger" @select="requestDelete(item)"><Trash2 :size="15" />删除文章</ContextMenuItem>
                </template>
              </ContextMenuContent>
            </ContextMenuPortal>
          </ContextMenuRoot>
          <p v-if="!entries.length && !busy" class="no-entries">{{ query ? '没有匹配的文章' : '暂无文章' }}</p>
        </div>
      </aside>
      <main class="main">
        <div v-if="busy || error || published || copiedLink" class="notices" aria-live="polite">
          <div v-if="busy" class="notice info"><LoaderCircle class="spinner" :size="16" />{{ busy }}</div>
          <div v-if="error" class="notice danger"><CircleAlert :size="17" /><span>{{ error }}</span><button class="dismiss" @click="error = ''" aria-label="关闭错误">×</button></div>
          <div v-if="copiedLink" class="notice success"><Check :size="17" /><span>文章链接已复制</span><button class="dismiss" @click="copiedLink = false" aria-label="关闭通知">×</button></div>
          <div v-if="published" class="notice success"><Check :size="17" /><span>已推送 {{ published.commit.slice(0, 7) }} · {{ deploymentState || '等待 Cloudflare 部署' }}</span><Button variant="ghost" :disabled="checkingDeployment" @click="refreshDeployment(published.commit, true)">刷新部署状态</Button><Button variant="ghost" @click="openPage('cloudflare')">部署记录 <ArrowUpRight :size="13" /></Button><Button v-if="published.path" variant="ghost" @click="openPage('article', published.path)">打开文章 <ArrowUpRight :size="13" /></Button><button class="dismiss" @click="published = undefined" aria-label="关闭通知">×</button></div>
        </div>
        <article v-if="active && !supportsEditing(active.markdown)" class="read-only-article"><h2>查看文章原文</h2><p class="setup-note">这篇文章的元数据为非 YAML 格式或解析失败。原文件保持原样。</p><pre>{{ active.markdown }}</pre></article>
        <template v-else-if="active">
          <div class="article-head">
            <div class="article-title"><input :value="text(fields.title)" aria-label="文章标题" :disabled="!!busy" placeholder="文章标题" @input="field('title', ($event.target as HTMLInputElement).value)" /><div class="article-path">{{ active.path }}</div></div>
            <div class="article-actions"><span class="save-state" aria-live="polite">{{ saveState }}</span>
              <Button variant="outline" title="显示或隐藏文章设置" :aria-pressed="metaOpen" @click="metaOpen = !metaOpen"><PanelLeft :size="16" /> 文章设置</Button><Button :disabled="!!busy" @click="prepare"><Send :size="15" /> 发布</Button></div>
          </div>
          <Transition name="metadata-fold">
            <div v-show="metaOpen" class="metadata-fold" :inert="!!busy || !metaOpen">
              <div class="metadata-clip">
                <div class="metadata">
                  <label>Slug<input v-model="slugInput" :readonly="!!active.originalPath" @change="updateSlug(slugInput)" /></label>
                  <TagField :key="active.id + '-categories'" label="分类" :values="listItems(fields.categories)" @change="field('categories', $event)" />
                  <TagField :key="active.id + '-tags'" label="标签" :values="listItems(fields.tags)" @change="field('tags', $event)" />
                  <label class="excerpt">摘要<input :value="text(fields.description)" placeholder="选填" @change="field('description', ($event.target as HTMLInputElement).value)" /></label>
                  <label class="cover">封面<span><input :value="text(fields.image)" placeholder="图片路径或网址" @change="field('image', ($event.target as HTMLInputElement).value)" /><Button variant="outline" title="粘贴封面" aria-label="粘贴封面" :disabled="!!busy" @click="imageAction('clipboard', 'cover')"><ImagePlus :size="16" /></Button></span></label>
                  <Button variant="ghost" class="yaml-button" @click="openYaml"><Fingerprint :size="15" /> YAML 源文</Button>
                </div>
              </div>
            </div>
          </Transition>
          <SplitterGroup direction="horizontal" class="split" :class="{ 'single-pane': viewMode !== 'split' }">
            <Transition name="pane-reveal">
              <SplitterPanel v-show="viewMode !== 'preview'" :default-size="49" :min-size="28" class="pane editor-pane">
                <EditorToolbar :disabled="!!busy" @format="format" @image="imageAction" />
                <ContextMenuRoot>
                  <ContextMenuTrigger as-child :disabled="!!busy">
                    <div class="editor-body" :inert="!!busy"><Editor :key="active.id" ref="editor" :dark="dark" :authoring="document?.body ?? active.markdown" @change="body" @save="saveDraft" @image="imageAction('clipboard')" /></div>
                  </ContextMenuTrigger>
                  <ContextMenuPortal>
                    <ContextMenuContent class="menu-content">
                      <ContextMenuItem class="menu-item" @select="refreshExternalMarkdown(true)"><RefreshCw :size="15" />刷新外部修改</ContextMenuItem>
                      <ContextMenuItem class="menu-item" @select="openExternalMarkdown"><ArrowUpRight :size="15" />用默认应用程序打开</ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenuPortal>
                </ContextMenuRoot>
              </SplitterPanel>
            </Transition>
            <SplitterResizeHandle v-show="viewMode === 'split'" class="resize-handle" aria-label="调整分栏宽度" />
            <Transition name="pane-reveal">
              <SplitterPanel v-show="viewMode !== 'edit'" :default-size="51" :min-size="28" class="pane preview-pane">
                <PreviewPane :key="active.id" :url="previewUrl" :revision="previewRevision" :loading="previewing" :busy="!!busy"
                  :error="previewError" @refresh="schedulePreview(0, true)" @open-browser="openPreviewBrowser" />
              </SplitterPanel>
            </Transition>
          </SplitterGroup>
        </template>
        <div v-else class="empty-workspace"><div class="empty-card"><span class="empty-symbol"><BookOpen :size="29" /></span><h2>打开一篇文章</h2><p>从左侧选择文章，或开始一篇新的 Markdown 草稿。</p><Button :disabled="!!busy" @click="create"><Plus :size="17" /> 新建文章</Button></div></div>
      </main>
    </div>
    <footer class="statusbar">
      <div class="status-group repository-status">
        <button type="button" class="repository-trigger" aria-haspopup="dialog" :aria-expanded="repositoryOpen" :title="'查看仓库与子模块 · ' + repoName" :disabled="!!busy && !repositoryRefreshing" @click="showRepository">
          <span class="status-repository">{{ repoName }}</span><span class="repository-dot" aria-hidden="true" />
        </button>
        <button type="button" class="status-button" :title="sidebarOpen ? '收起文章目录' : '展开文章目录'" :aria-label="sidebarOpen ? '收起文章目录' : '展开文章目录'" :aria-expanded="sidebarOpen" aria-controls="article-sidebar" @click="sidebarOpen = !sidebarOpen">
          <PanelLeftClose v-if="sidebarOpen" :size="16" /><PanelLeftOpen v-else :size="16" />
        </button>
      </div>
      <div v-if="active && supportsEditing(active.markdown)" class="status-group">
        <div class="view-controls" role="group" aria-label="编辑器布局">
          <button v-for="option in viewOptions" :key="option.value" type="button" class="status-button" :title="option.label" :aria-label="option.label" :aria-pressed="viewMode === option.value" @click="viewMode = option.value">
            <component :is="option.icon" :size="15" />
          </button>
        </div>
        <span>{{ words }} 字词</span><span>UTF-8 · CRLF</span><span>Hugo<span v-if="previewPort"> · {{ previewPort }}</span></span>
      </div>
    </footer>
    <SettingsDialog v-model:open="settingsOpen" :preferences="preferences" :hosted-available="site.hostedImages" :preview-port="previewPort" @saved="applySettings" />
    <input ref="imageFile" hidden type="file" accept="image/png,image/jpeg,image/webp,image/bmp,image/gif" @change="importFile" @cancel="pendingImage = undefined" />
    <Dialog v-model:open="repositoryOpen" title="站点仓库" description="刷新会同步博客和子模块，并用子模块的远端版本更新本地 Hugo 预览。">
      <section class="repository-details" aria-label="博客仓库">
        <strong>博客仓库</strong><code>{{ repositoryInfo?.remote ?? site.remote }}</code>
      </section>
      <section class="repository-details" aria-label="Git 子模块">
        <strong>Git 子模块</strong>
        <span v-if="repositoryLoading" role="status">正在读取子模块…</span>
        <span v-else-if="repositoryInfo && !repositoryInfo.modules.length">此仓库没有 Git 子模块。</span>
        <ul v-else-if="repositoryInfo" class="repository-modules">
          <li v-for="module in repositoryInfo.modules" :key="module.path"><span>{{ module.path }}</span><code>{{ module.url }}</code></li>
        </ul>
      </section>
      <p v-if="repositoryError" class="danger-text repository-feedback" role="alert">{{ repositoryError }}</p>
      <p v-else-if="repositoryMessage || repositoryRefreshing" class="repository-feedback" role="status">{{ repositoryRefreshing ? '正在刷新仓库和 Hugo 预览…' : repositoryMessage }}</p>
      <div class="dialog-actions">
        <Button :disabled="!!busy || repositoryLoading" title="刷新(包含子模块)" @click="refreshRepository">
          <LoaderCircle v-if="repositoryRefreshing" class="spinner" :size="15" /><RefreshCw v-else :size="15" />刷新(包含子模块)
        </Button>
      </div>
    </Dialog>
    <Dialog v-model:open="externalConflictOpen" title="导入外部修改？" description="CMS 草稿和外部 Markdown 都有改动。导入会用外部文件内容替换当前草稿。">
      <div class="dialog-actions"><Button variant="outline" @click="externalConflictOpen = false">取消</Button><Button @click="confirmExternalMarkdown">导入外部修改</Button></div>
    </Dialog>
    <Dialog v-model:open="imageUrlOpen" title="插入图片网址" description="将在线图片插入正文光标处。">
      <label class="dialog-field">图片网址<input v-model="imageUrl" type="url" placeholder="https://example.com/photo.png" @keydown.enter="insertImageUrl" /></label>
      <p v-if="imageUrlError" class="danger-text">{{ imageUrlError }}</p>
      <div class="dialog-actions"><Button variant="outline" @click="imageUrlOpen = false">取消</Button><Button @click="insertImageUrl">插入图片</Button></div>
    </Dialog>
    <Dialog v-model:open="discardOpen"
      :title="discardTarget?.draft?.originalPath ? '放弃本地修改？' : '删除草稿？'"
      :description="discardTarget?.draft?.originalPath ? '《' + discardTarget.title + '》将恢复为原文。' : '《' + (discardTarget?.title ?? '') + '》及草稿图片将从本地草稿列表移除。'">
      <div class="dialog-actions"><Button variant="outline" @click="discardOpen = false">取消</Button><Button class="button-danger" @click="discard">{{ discardTarget?.draft?.originalPath ? '放弃本地修改' : '删除草稿' }}</Button></div>
    </Dialog>
    <Dialog v-model:open="deleteOpen" title="删除文章？" :description="'将从 GitHub 的 ' + site.branch + ' 分支删除文章文件。Cloudflare 重新部署后，线上文章会下线。'">
      <p v-if="deleteTarget" class="delete-article-target"><strong>{{ itemTitle(deleteTarget.markdown, deleteTarget.path) }}</strong><code>{{ deleteTarget.path }}</code></p>
      <p v-if="drafts.some(draft => draft.originalPath === deleteTarget?.path)" class="setup-note">这篇文章的本地修改仍保存在草稿中。</p>
      <div class="dialog-actions"><Button variant="outline" @click="deleteOpen = false">取消</Button><Button class="button-danger" @click="removeArticle">删除并推送</Button></div>
    </Dialog>
    <Dialog v-model:open="yamlOpen" title="文章源文" description="编辑 YAML 元数据与 Markdown 正文。">
      <textarea v-model="yamlBuffer" class="yaml-source" aria-label="YAML 和 Markdown 源文" /><p v-if="yamlError" class="danger-text">{{ yamlError }}</p>
      <div class="dialog-actions"><Button variant="outline" @click="yamlOpen = false">取消</Button><Button @click="saveYaml">应用</Button></div>
    </Dialog>
    <Dialog v-model:open="publishOpen" title="确认发布" :description="'推送 ' + site.branch + ' 后，Cloudflare 将开始构建网站。'">
      <div v-if="summary" class="publish-summary"><p>{{ summary.url }}</p><ul><li v-for="file in summary.files" :key="file">{{ file }}</li><li v-for="file in summary.hostedFiles" :key="file">图床 · {{ file }}</li></ul><details v-if="summary.diff"><summary>查看文章改动</summary><pre>{{ summary.diff }}</pre></details></div>
      <div class="dialog-actions"><Button variant="outline" @click="publishOpen = false">取消</Button><Button @click="publish"><Send :size="15" /> 推送 {{ site.branch }}</Button></div>
    </Dialog>
  </div>
</template>
