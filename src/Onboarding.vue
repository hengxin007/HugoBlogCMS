<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { ArrowLeft, ArrowRight, ArrowUpRight, BookOpen, Check, ChevronRight, Cloud, CodeXml, Copy, FolderOpen, Github, Globe2, LoaderCircle, Plus, Settings2 } from 'lucide-vue-next'
import Button from './components/ui/Button.vue'
import type { Route, SetupInput, Site, Startup } from './onboarding'

const props = defineProps<{ initial: Startup }>()
const emit = defineEmits<{ finished: [site: Site] }>()
const state = ref(props.initial)
const wizard = computed(() => state.value.wizard)
const form = ref<SetupInput>()
const welcome = ref(true)
const loading = ref('')
const error = ref('')
const githubToken = ref('')
const pagesToken = ref('')
const pagesMode = ref<'existing' | 'new'>('existing')
const preview = ref<{ home: string; article: string | null }>()
const copied = ref('')
const creating = computed(() => wizard.value?.route === 'create')
const labels = computed(() => creating.value ? ['创建 Hugo 站点', '连接 GitHub', '部署到 Cloudflare'] : ['连接 GitHub', '检查 Hugo 站点', '关联 Cloudflare'])
const icons = computed(() => creating.value ? [CodeXml, Github, Cloud] : [Github, CodeXml, Cloud])
const step = computed(() => wizard.value?.step ?? 1)
const githubStep = computed(() => creating.value ? step.value === 2 : step.value === 1)
const buildStep = computed(() => creating.value ? step.value === 1 : step.value === 2)
const repository = computed(() => form.value?.remote.replace(/^https:\/\/github\.com\//, '').replace(/\.git$/, ''))
const hasPreview = computed(() => !!wizard.value && (creating.value ? step.value >= 2 : !!wizard.value.hugoVersion && step.value >= 2))
const buildRows = computed(() => [
  { label: '生产分支', value: form.value?.branch || 'main' },
  { label: '构建命令', value: creating.value ? state.value.pagesBuild : 'hugo' },
  { label: '输出目录', value: 'public' },
  { label: 'HUGO_VERSION', value: wizard.value?.hugoVersion || '' },
  ...(!creating.value && form.value?.rootDir ? [{ label: '根目录', value: form.value.rootDir }] : []),
])
watch(wizard, value => { form.value = value ? { ...value.input } : undefined }, { immediate: true })

async function act(action: string | { start: Route }, label: string) {
  if (loading.value) return
  error.value = ''
  loading.value = label
  try {
    state.value = await invoke<Startup>('wizard_action', {
      action, input: typeof action === 'string' && action !== 'finish' ? form.value : null,
      githubToken: githubToken.value || null, pagesToken: pagesToken.value || null,
    })
    githubToken.value = ''
    pagesToken.value = ''
    preview.value = undefined
    if (state.value.site && (action === 'finish' || action === 'continue')) emit('finished', state.value.site)
    else welcome.value = false
  } catch (reason) { error.value = String(reason) }
  finally { loading.value = '' }
}
function start(route: Route) {
  const previous = form.value?.remote
  githubToken.value = ''
  pagesToken.value = ''
  pagesMode.value = 'existing'
  if (previous && route === 'import' && !welcome.value && wizard.value?.route === 'create') {
    void act({ start: route }, '正在准备向导…').then(() => { if (form.value) form.value.remote = previous })
    return
  }
  void act({ start: route }, '正在准备向导…')
}
async function showWelcome() {
  if (form.value && wizard.value && wizard.value.step < 4) {
    await act('save', '正在保存设置…')
    if (error.value) return
  }
  error.value = ''
  preview.value = undefined
  githubToken.value = ''
  pagesToken.value = ''
  welcome.value = true
}
async function invokeUI(command: string, args: Record<string, unknown>, done?: (value: any) => void) {
  if (loading.value) return
  error.value = ''
  loading.value = '处理中…'
  try { const value = await invoke(command, args); done?.(value) }
  catch (reason) { error.value = String(reason) }
  finally { loading.value = '' }
}
function browser(page: string) { return invokeUI('setup_browser', { page }) }
function seePreview() { return invokeUI('setup_preview', {}, value => { preview.value = value }) }
async function copy(label: string, value: string) {
  try { await navigator.clipboard.writeText(value); copied.value = label }
  catch { error.value = '无法写入剪贴板，可以选中文本手动复制。' }
}
function push() {
  void act('push', '正在核对仓库并推送…')
}
function connect() {
  void act('connect', '正在拉取站点和主题…')
}
</script>

<template>
  <div class="setup-shell" :aria-busy="!!loading">
    <header class="setup-appbar">
      <div class="brand"><span class="brand-mark"><BookOpen :size="21" /></span><div><strong>博客 CMS</strong><span>站点设置</span></div></div>
      <Button v-if="!welcome" variant="ghost" :disabled="!!loading" @click="showWelcome"><ArrowLeft :size="16" /> 返回欢迎页</Button>
    </header>

    <main v-if="welcome" class="setup-welcome">
      <div class="setup-hero-icon"><BookOpen :size="32" :stroke-width="1.65" /></div>
      <h1>欢迎使用博客 CMS</h1>
      <p v-if="state.site">已连接 {{ state.site.title }}。可以继续写作，或检查 GitHub 和 Cloudflare 设置。</p>
      <p v-else>创建你的 Hugo 博客，或连接一个已有站点。</p>
      <div class="setup-options">
        <button v-if="state.site" class="setup-card" type="button" :disabled="!!loading" @click="act('continue', '正在返回写作…')">
          <span class="setup-card-icon"><BookOpen :size="25" :stroke-width="1.8" /></span>
          <strong>继续使用现有站点</strong>
          <span>打开写作界面，接着编辑已有文章和草稿。</span>
          <span class="setup-card-link">进入写作 <ArrowRight :size="17" /></span>
        </button>
        <button v-else class="setup-card" type="button" :disabled="!!loading" @click="start('create')">
          <span class="setup-card-icon"><Plus :size="25" :stroke-width="1.8" /></span>
          <strong>创建新博客</strong>
          <span>生成 Hugo 站点，连接 GitHub，并部署到 Cloudflare Pages。</span>
          <span class="setup-card-link">开始创建 <ArrowRight :size="17" /></span>
        </button>
        <button class="setup-card" type="button" :disabled="!!loading" @click="start('import')">
          <span class="setup-card-icon"><FolderOpen :size="24" :stroke-width="1.8" /></span>
          <strong>{{ state.site ? '配置现有站点' : '导入已有站点' }}</strong>
          <span>{{ state.site ? '核对当前站点的 GitHub 仓库与 Cloudflare Pages 项目。' : '使用现有 GitHub 仓库，沿用站点主题、内容和文章链接。' }}</span>
          <span class="setup-card-link">{{ state.site ? '打开设置' : '开始导入' }} <ArrowRight :size="17" /></span>
        </button>
      </div>
      <div class="setup-actions welcome-actions">
        <Button v-if="wizard" :disabled="!!loading" @click="welcome = false">继续{{ state.site ? '配置站点' : wizard.route === 'create' ? '创建' : '导入' }} <ChevronRight :size="16" /></Button>

      </div>
      <p v-if="loading" class="setup-feedback" role="status"><LoaderCircle class="spinner" :size="17" /> {{ loading }}</p>
      <pre v-if="error" class="setup-error" role="alert">{{ error }}</pre>
    </main>

    <main v-else-if="wizard && form" class="setup-wizard">
      <aside class="setup-sidebar">
        <strong>{{ creating ? '创建新博客' : '导入已有站点' }}</strong>
        <ol aria-label="设置进度">
          <li v-for="(label, index) in labels" :key="label" :class="{ current: step === index + 1, complete: step > index + 1 }" :aria-current="step === index + 1 ? 'step' : undefined">
            <span class="setup-step-icon"><Check v-if="step > index + 1" :size="16" /><component :is="icons[index]" v-else :size="16" /></span>{{ label }}
          </li>
        </ol>

      </aside>
      <section class="setup-panel">
        <div class="setup-panel-body">
          <div :key="step" class="setup-step-content">
            <template v-if="step === 4 && wizard.checked">
              <div class="setup-card-icon"><Check :size="26" /></div>
              <h1>可以开始写作了</h1>
              <p>{{ wizard.deployment }}</p>
              <dl class="setup-summary"><dt>博客</dt><dd>{{ wizard.checked.title }}</dd><dt>源码仓库</dt><dd>{{ repository }} · {{ wizard.checked.branch }}</dd><dt>访问地址</dt><dd>{{ wizard.checked.website }}</dd></dl>
              <Button variant="outline" :disabled="!!loading" @click="browser('website')"><Globe2 :size="16" /> 打开我的博客 <ArrowUpRight :size="14" /></Button>
            </template>
            <template v-else>
              <h1>{{ labels[step - 1] }}</h1>
              <fieldset :disabled="!!loading">
                <template v-if="githubStep">
                  <p>{{ creating ? '在 GitHub 创建一个空仓库，再把仓库地址填到下面。' : '填入存放 Hugo 源码的 GitHub 仓库。导入只拉取内容和主题。' }}</p>
                  <div v-if="creating" class="setup-inline"><Button variant="outline" @click="browser('github_new')"><Github :size="16" /> 打开 GitHub 创建仓库 <ArrowUpRight :size="14" /></Button><small>请关闭初始化 README、.gitignore 和许可证选项。</small></div>
                  <label class="setup-field">GitHub 仓库<input v-model.trim="form.remote" :readonly="!!state.site" placeholder="owner/blog 或 https://github.com/owner/blog" spellcheck="false" /></label>
                  <label v-if="!creating" class="setup-field">发布分支 <small v-if="!state.site">可留空，自动使用仓库默认分支</small><input v-model.trim="form.branch" :readonly="!!state.site" placeholder="例如 main" spellcheck="false" /></label>
                  <div v-else class="setup-note">首次推送目标分支为 <code>main</code>。</div>
                  <details class="setup-details">
                    <summary><Settings2 :size="16" /> 授权和网络设置</summary>
                    <p>默认由 Git Credential Manager 在浏览器中登录。手动授权可使用仅限目标仓库、具备 Contents 读写权限的细粒度令牌。</p>
                    <label class="setup-field">GitHub 令牌 <small>可选</small><input v-model="githubToken" type="password" autocomplete="off" placeholder="留空使用浏览器登录或已保存的凭据" /></label>
                    <Button variant="ghost" @click="browser('github_token')">创建 GitHub 令牌 <ArrowUpRight :size="14" /></Button>
                    <label v-if="!creating" class="setup-field">HTTP / HTTPS 代理 <small>可选</small><input v-model.trim="form.proxy" placeholder="http://127.0.0.1:7897" /></label>
                  </details>
                </template>
                <template v-if="buildStep">
                  <p>{{ creating ? '使用随软件提供的 Hugo Extended 和博客模板。' : '核对识别结果，确认现有主题可以在本机正常构建。' }}</p>
                  <label class="setup-field">博客名称<input v-model.trim="form.title" maxlength="100" placeholder="我的 Hugo 博客" /></label>
                  <template v-if="!creating">
                    <label class="setup-field">Hugo 站点根目录 <small>相对于仓库，根目录可留空</small><input v-model.trim="form.rootDir" :readonly="!!state.site" @change="form.postsDir = ''; form.website = ''; form.title = ''" placeholder="例如 website" /></label>
                    <label class="setup-field">文章目录 <small>相对于仓库</small><input v-model.trim="form.postsDir" :readonly="!!state.site" list="setup-post-folders" placeholder="例如 content/posts" /><datalist id="setup-post-folders"><option v-for="folder in wizard.candidates" :key="folder" :value="folder" /></datalist></label>
                    <label class="setup-field">正式访问地址 <small>未配置域名可留空</small><input v-model.trim="form.website" placeholder="https://你的域名" /></label>
                    <div v-if="wizard.hugoVersion" class="setup-note">{{ wizard.theme || '站点自带布局' }} · 发现 {{ wizard.articles }} 篇文章<span v-if="wizard.unsupported">（{{ wizard.unsupported }} 篇仅查看原文）</span></div>
                    <Button variant="outline" @click="act('detect', '正在识别 Hugo 配置…')">重新识别</Button>
                  </template>
                  <details class="setup-details">
                    <summary><Settings2 :size="16" /> Hugo 和网络设置</summary>
                    <label class="setup-field">Hugo 可执行文件 <small>可选</small><span class="setup-inline"><input v-model.trim="form.hugoPath" placeholder="留空使用内置版本" /><Button variant="outline" @click="invokeUI('pick_hugo', {}, value => { if (value && form) form.hugoPath = value })">浏览</Button></span></label>
                    <label class="setup-field">HTTP / HTTPS 代理 <small>可选</small><input v-model.trim="form.proxy" placeholder="http://127.0.0.1:7897" /></label>
                  </details>
                </template>
                <template v-if="step === 3">
                  <p v-if="creating">在 Cloudflare 授权访问 <strong>{{ repository }}</strong>，连接仓库并创建 Pages 项目。</p>
                  <template v-else>
                    <p>关联这个站点的生产部署。向导会读取站点和项目状态。</p>
                    <div class="setup-choice" role="group" aria-label="Cloudflare 配置方式">
                      <Button :variant="pagesMode === 'existing' ? 'default' : 'outline'" :aria-pressed="pagesMode === 'existing'" @click="pagesMode = 'existing'">关联已有项目</Button>
                      <Button :variant="pagesMode === 'new' ? 'default' : 'outline'" :aria-pressed="pagesMode === 'new'" @click="pagesMode = 'new'">配置 Pages</Button>
                    </div>
                    <p v-if="pagesMode === 'new'">在 Cloudflare 选择同一个 GitHub 仓库，按下面的参数创建 Pages 项目。</p>
                  </template>
                  <Button variant="outline" @click="browser('pages')"><Cloud :size="16" /> 打开 Cloudflare <ArrowUpRight :size="14" /></Button>
                  <div v-if="creating || pagesMode === 'new'" class="setup-build-table" aria-label="Pages 构建配置">
                    <div v-for="row in buildRows" :key="row.label"><span>{{ row.label }}</span><code>{{ row.value }}</code><Button variant="ghost" :aria-label="'复制' + row.label" @click="copy(row.label, row.value)"><Check v-if="copied === row.label" :size="15" /><Copy v-else :size="15" /></Button></div>
                  </div>
                  <label class="setup-field">Pages 项目名 <small>可从下方地址推断</small><input v-model.trim="form.pagesProject" placeholder="例如 blog" /></label>
                  <label class="setup-field">Pages 生产地址<input v-model.trim="form.pagesUrl" placeholder="https://your-blog.pages.dev/" spellcheck="false" /></label>
                  <label v-if="!creating" class="setup-check"><input v-model="form.confirmed" type="checkbox" />我已在 Cloudflare 确认此项目连接 {{ repository }}，生产分支为 {{ form.branch }}</label>
                  <details class="setup-details">
                    <summary><Settings2 :size="16" /> 使用 Pages Read 令牌核对项目</summary>
                    <label class="setup-field">Cloudflare 账号 ID<input v-model.trim="form.account" spellcheck="false" /></label>
                    <label class="setup-field">Cloudflare API 令牌<input v-model="pagesToken" type="password" autocomplete="off" placeholder="只需 Pages Read 权限" /></label>
                    <Button variant="ghost" @click="browser('pages_token')">打开令牌设置 <ArrowUpRight :size="14" /></Button>
                  </details>
                </template>
              </fieldset>
            </template>
          </div>
          <div v-if="hasPreview && step < 4" class="setup-preview-actions">
            <Button v-if="!preview" variant="outline" :disabled="!!loading" @click="seePreview"><Globe2 :size="16" /> 准备本地预览</Button>
            <template v-else>
              <Button variant="outline" :disabled="!!loading" @click="invokeUI('open_preview', { url: preview?.home })">预览首页 <ArrowUpRight :size="14" /></Button>
              <Button v-if="preview.article" variant="outline" :disabled="!!loading" @click="invokeUI('open_preview', { url: preview?.article })">预览文章 <ArrowUpRight :size="14" /></Button>
            </template>
          </div>
          <div v-if="loading" class="setup-feedback" role="status"><LoaderCircle :size="17" class="spinner" />{{ loading }}</div>
          <div v-if="error" class="setup-feedback" role="alert">
            <pre class="setup-error">{{ error }}</pre>
            <Button v-if="error.includes('找不到 Git')" variant="outline" @click="browser('git')">下载 Git for Windows <ArrowUpRight :size="14" /></Button>
            <Button v-if="creating && error.includes('仓库已有内容')" variant="outline" @click="start('import')">改为导入已有站点</Button>
          </div>
        </div>
        <footer class="setup-footer">
          <Button variant="ghost" :disabled="!!loading" @click="step === 1 ? showWelcome() : act('back', '正在返回…')"><ArrowLeft :size="16" /> 上一步</Button>
          <Button v-if="step === 4" :disabled="!!loading" @click="act('finish', '正在打开写作区…')">进入写作 <ArrowRight :size="16" /></Button>
          <Button v-else-if="step === 3" :disabled="!!loading" @click="act('verify', '正在验证生产部署…')">{{ error ? '重新验证' : '验证部署' }} <ArrowRight :size="16" /></Button>
          <Button v-else-if="githubStep" :disabled="!!loading" @click="creating ? push() : connect()">{{ creating ? '确认首次推送' : '拉取并继续' }} <ArrowRight :size="16" /></Button>
          <Button v-else :disabled="!!loading" @click="creating ? act('generate', '正在生成并构建 Hugo 站点…') : act('inspect', '正在验证 Hugo 构建…')">{{ creating ? '生成并验证站点' : '验证构建并继续' }} <ArrowRight :size="16" /></Button>
        </footer>
      </section>
    </main>
  </div>
</template>
