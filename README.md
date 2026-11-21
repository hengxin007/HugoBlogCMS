# HugoBlogCMS

博客 CMS 是一款 Windows 桌面写作工具，用于创建和导入 Hugo 博客。在客户端编辑 Markdown、使用站点原主题预览文章，再将确认后的改动推送到 GitHub，由关联的 Cloudflare Pages 项目部署。

## 功能

- 通过向导创建 Hugo 站点，或导入已有的 GitHub 源码仓库、主题和文章。
- 编辑 Markdown 和 YAML 元数据，使用可移除的分类和标签，粘贴剪贴板图片或导入本地图片。
- 自动保存本地草稿。正文获得焦点时，也可以按 `Ctrl+S` 立即保存。
- 在编辑、预览和左右分栏之间切换，或用默认浏览器打开 Hugo 预览。
- 通过 Markdown 右键菜单，用 Windows 默认关联的应用编辑本地草稿副本。CMS 会读回当前文章的外部改动，并在双方都有改动时要求确认。
- 发布前查看改动和目标地址，确认后推送。推送成功后可查看 Cloudflare Pages 部署状态。

## Windows 安装

运行已取得的 Windows x64 安装程序，再从开始菜单打开「博客 CMS」。安装包随附 Hugo Extended。首次连接 GitHub 前，请安装 Git for Windows 并保留 Git Credential Manager。也可以按下方「开发与打包」步骤自行生成安装包。

## 首次设置

1. **创建新博客**：在向导中生成站点，到 GitHub 创建空仓库，回到 CMS 确认首次推送。在 Cloudflare Pages 连接仓库，填写向导提供的构建命令、输出目录和 Hugo 版本，再回到 CMS 验证部署。
2. **导入已有站点**：填写现有 Hugo 源码仓库地址，核对发布分支、站点根目录、文章目录和正式访问地址。CMS 在本地工作副本中读取和构建原主题，导入阶段只读取 GitHub 和 Cloudflare。填写已有 Pages 项目，或按向导新建项目。
3. **开始写作**：打开已有文章或新建草稿。点「发布」查看摘要，确认后推送到站点的发布分支。

GitHub 默认通过 Git Credential Manager 登录。手动填写的 GitHub 或 Cloudflare 令牌保存在 Windows 凭据管理器。需要代理时，可在向导或「应用设置 → 通用」填写 HTTP 或 HTTPS 代理。

## 数据备份与恢复

「应用设置 → 通用 → 数据与备份」显示实际数据目录。安装版使用 `hugo-blog-cms.exe` 旁边的 `data`，开发模式使用项目根目录的 `data`。

- `data/cms.sqlite3` 保存站点与应用设置、向导进度、草稿正文和发布记录。
- `data/source`、`data/drafts` 和 `data/onboarding` 存放 Hugo 工作副本、草稿文件及向导工作副本。
- 备份前请关闭 CMS，再复制整个 `data` 目录。恢复时先关闭 CMS，用完整备份替换当前 `data`。换电脑后，按需重新授权并核对自定义 Hugo 路径。

## 开发与打包

需要 Windows x64、PowerShell 7、Node.js 22.18 或更新的兼容版本、Git for Windows、Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK 和 WebView2。

在仓库根目录打开 PowerShell，安装依赖并准备 Hugo：

```powershell
npm ci
pwsh -File scripts/prepare-hugo.ps1
```

准备脚本下载官方 Hugo Extended 0.130.0，校验 SHA-256 后将可执行文件放入 Tauri 的 sidecar 目录。脚本支持使用 `-Proxy` 参数指定下载代理。

```powershell
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

生成 Windows x64 安装包：

```powershell
npm run tauri build -- --bundles nsis
```

默认输出目录为 `src-tauri/target/release/bundle/nsis/`。如果设置了 `CARGO_TARGET_DIR`，则在该目录下的 `release/bundle/nsis/` 查找安装包。

## 许可

HugoBlogCMS 源码采用 [MIT 许可](LICENSE)。随安装包分发的 Hugo 保留其 [Apache 2.0 许可](src-tauri/licenses/Hugo-LICENSE)。
