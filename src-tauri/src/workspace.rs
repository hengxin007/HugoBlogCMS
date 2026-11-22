use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::{NamedTempFile, TempDir};
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;

pub mod external_edit;
mod git_proxy;
pub mod onboarding;
pub mod repository;
use onboarding::SiteConfig;
mod storage;
use storage::{Storage, DRAFTS, OPERATIONS, SETTINGS};

const BLOG: &str = "https://github.com/hengxin007/blog.git";

const HOSTING: &str = "https://github.com/hengxin007/picx-images-hosting.git";
const SITE: &str = "https://544688.xyz/";
type Reply<T> = Result<T, String>;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    id: String,
    path: String,
    original_path: Option<String>,
    original_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle_origin: Option<String>,
    markdown: String,
    images: Vec<Picture>,
}
impl Draft {
    pub fn with_markdown(&self, markdown: String) -> Self {
        let mut draft = self.clone();
        draft.markdown = markdown;
        draft
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Picture {
    id: String,
    path: String,
    hosted: bool,
}
#[derive(Debug, Serialize)]
pub struct Article {
    path: String,
    markdown: String,
    hash: String,
}
#[derive(Serialize)]
pub struct Library {
    articles: Vec<Article>,
    drafts: Vec<Draft>,
    warning: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    url: String,
    files: Vec<String>,
    hosted_files: Vec<String>,
    diff: String,
    approval: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Published {
    path: String,
    commit: String,
}

#[derive(Debug, Serialize)]
pub struct PublishResult {
    #[serde(flatten)]
    published: Published,
    article: Article,
}

#[derive(Default, Deserialize, Serialize)]
struct Journal {
    commit: Option<String>,
    hosted_commit: Option<String>,
    published: Option<Published>,
}

struct Preview {
    child: Child,
    dir: TempDir,
    log: PathBuf,
    origin: String,
    id: String,
    path: String,
    revision: String,
    updated_modules: bool,
    images: HashSet<String>,
}
impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    disable_gpu: bool,
    disable_animations: bool,
    proxy: String,
    hugo_path: String,
    preview_port: Option<u16>,
    hosted_images: Option<bool>,
    editor_font_size: u8,
    interface_font_size: u8,
    theme: Theme,
    accent_color: String,
    custom_base: ThemeBase,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    #[default]
    System,
    Light,
    Dark,
    Custom,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ThemeBase {
    #[default]
    System,
    Light,
    Dark,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            disable_gpu: false,
            disable_animations: false,
            proxy: String::new(),
            hugo_path: String::new(),
            preview_port: None,
            hosted_images: None,
            editor_font_size: 13,
            interface_font_size: 14,
            theme: Theme::System,
            accent_color: "#a5235e".into(),
            custom_base: ThemeBase::System,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HugoInfo {
    path: String,
    version: String,
    extended: bool,
}

pub struct Workspace {
    root: PathBuf,
    storage: Storage,
    bundled_hugo: PathBuf,
    site: Option<SiteConfig>,
    preferences: Preferences,

    blog_remote: String,

    hosting_remote: String,
    preview: Option<Preview>,
    preview_modules: Option<repository::PreviewModules>,
    approval: Option<(String, String, String)>,
}

fn error(err: impl std::fmt::Display) -> String {
    err.to_string()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id)
}
fn path_text(path: &Path) -> Reply<&str> {
    path.to_str()
        .ok_or_else(|| "路径必须可以转换为 UTF-8。".into())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Reply<()> {
    let parent = path.parent().ok_or("缺少上级目录。")?;
    fs::create_dir_all(parent).map_err(error)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(error)?;
    temporary.write_all(bytes).map_err(error)?;
    temporary.as_file().sync_all().map_err(error)?;
    temporary.persist(path).map_err(error)?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Reply<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(error),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(error(err)),
    }
}

fn crlf(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}
fn safe_join(base: &Path, relative: &str) -> Reply<PathBuf> {
    if relative.contains(['\\', ':', '\0'])
        || relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' ']))
    {
        return Err("文件路径无效。".into());
    }
    let mut target = base.to_path_buf();
    for part in relative.split('/') {
        target.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = metadata.file_type().is_symlink();
            if redirected {
                return Err("工作区路径包含重定向。".into());
            }
        }
    }
    Ok(target)
}
fn is_section_index(path: &Path) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "_index" || name.starts_with("_index."))
}
fn article_path(base: &Path, path: &str, posts: &str) -> Reply<PathBuf> {
    if !path
        .strip_prefix(posts)
        .is_some_and(|part| part.starts_with('/') && part.len() > 1)
        || !path.ends_with(".md")
        || is_section_index(Path::new(path))
    {
        return Err("文章路径必须位于站点配置的文章目录内。".into());
    }
    safe_join(base, path)
}
fn validate(draft: &Draft, posts: &str) -> Reply<()> {
    if !uuid(&draft.id) || draft.markdown.len() > 8 * 1024 * 1024 || draft.images.len() > 250 {
        return Err("草稿 ID、长度或图片数量无效。".into());
    }
    if let Some(original) = &draft.original_path {
        if original != &draft.path
            || draft
                .original_hash
                .as_ref()
                .is_none_or(|item| item.len() != 64)
        {
            return Err("原有文章的路径不可修改。".into());
        }
    } else {
        let slug = draft
            .path
            .strip_prefix(posts)
            .and_then(|p| p.strip_prefix('/'))
            .and_then(|p| p.strip_suffix("/index.md"))
            .unwrap_or("");
        let reserved = ["con", "prn", "aux", "nul"].contains(&slug)
            || (slug.len() == 4
                && (slug.starts_with("com") || slug.starts_with("lpt"))
                && slug.as_bytes()[3].is_ascii_digit());
        if slug.is_empty()
            || slug.len() > 90
            || slug.starts_with('-')
            || slug.ends_with('-')
            || reserved
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("Slug 必须使用有效的小写字母、数字或连字符。".into());
        }
    }
    let mut ids = HashSet::new();
    for image in &draft.images {
        if !uuid(&image.id)
            || !ids.insert(&image.id)
            || image.path != picture_path(draft, &image.id)
        {
            return Err("草稿图片路径无效。".into());
        }
    }
    Ok(())
}
fn picture_path(draft: &Draft, id: &str) -> String {
    if draft.path.ends_with("/index.md") {
        format!("images/{id}.png")
    } else {
        format!("/cms-images/{}/{id}.png", draft.id)
    }
}
fn local_asset(draft: &Draft, picture: &Picture) -> String {
    if picture.path.starts_with('/') {
        format!("static{}", picture.path)
    } else {
        format!(
            "{}/{}",
            draft
                .path
                .rsplit_once('/')
                .expect("validated article path")
                .0,
            picture.path
        )
    }
}
fn hosted_asset(draft: &Draft, picture: &Picture) -> String {
    format!("hugo-cms/{}/{}.png", draft.id, picture.id)
}
fn hosted_url(revision: &str, draft: &Draft, picture: &Picture) -> String {
    format!(
        "https://cdn.jsdelivr.net/gh/hengxin007/picx-images-hosting@{revision}/{}",
        hosted_asset(draft, picture)
    )
}
fn active_images(draft: &Draft) -> impl Iterator<Item = &Picture> {
    draft
        .images
        .iter()
        .filter(|picture| draft.markdown.contains(&picture.path))
}
fn process(executable: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.stdin(Stdio::null());
    command
}
fn bind_preview_port(port: u16) -> Reply<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|err| {
        if port == 0 {
            format!("无法自动分配预览端口：{err}")
        } else {
            format!("无法使用预览端口 {port}，请在设置中换一个端口：{err}")
        }
    })
}
fn run(mut command: Command, label: &str) -> Reply<String> {
    let result = command.output().map_err(|err| format!("{label}：{err}"))?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    if result.status.success() {
        return Ok(stdout.into_owned());
    }
    Err(format!(
        "{label}失败：\n{}\n{}",
        String::from_utf8_lossy(&result.stderr).trim(),
        stdout.trim()
    ))
}

impl Workspace {
    pub fn open(root: PathBuf, legacy: &Path, hugo: PathBuf) -> Reply<Self> {
        storage::migrate(&root, legacy)?;
        Self::new(root, hugo)
    }

    #[cfg(windows)]
    pub fn disable_gpu(&self) -> bool {
        self.preferences.disable_gpu
    }

    pub fn data_directory(&self) -> Reply<String> {
        Ok(path_text(&self.root)?.to_owned())
    }

    pub fn new(root: PathBuf, hugo: PathBuf) -> Reply<Self> {
        fs::create_dir_all(root.join("drafts")).map_err(error)?;
        let storage = Storage::open(&root)?;
        let preferences = match storage.get(SETTINGS, "preferences")? {
            Some(preferences) => preferences,
            None => Preferences {
                proxy: storage.get(SETTINGS, "proxy")?.unwrap_or_default(),
                ..Preferences::default()
            },
        };
        let site = storage.get(SETTINGS, "site")?;
        Ok(Self {
            root,
            storage,
            bundled_hugo: hugo,
            preferences,

            blog_remote: site
                .as_ref()
                .map_or(BLOG, |site: &SiteConfig| site.remote.as_str())
                .to_owned(),

            hosting_remote: HOSTING.into(),
            site,
            preview: None,
            preview_modules: None,
            approval: None,
        })
    }
    pub fn settings(&mut self, changes: Option<Preferences>) -> Reply<Preferences> {
        if let Some(mut changes) = changes {
            changes.proxy = changes.proxy.trim().to_owned();
            changes.hugo_path = changes.hugo_path.trim().to_owned();
            if !changes.proxy.is_empty() {
                let url =
                    Url::parse(&changes.proxy).map_err(|_| "请输入 HTTP 或 HTTPS 代理地址。")?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || url.path() != "/"
                {
                    return Err("请输入不带账号和路径的代理地址。".into());
                }
            }
            if changes.preview_port.is_some_and(|port| port < 1024) {
                return Err("预览端口应为 1024–65535，留空可自动分配。".into());
            }
            if !(10..=30).contains(&changes.editor_font_size)
                || !(12..=20).contains(&changes.interface_font_size)
            {
                return Err("编辑器字号应为 10–30 px，界面字号应为 12–20 px。".into());
            }
            if changes.accent_color.len() != 7
                || !changes.accent_color.starts_with('#')
                || !changes.accent_color[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("请选择有效的主题颜色。".into());
            }
            let hugo_changed = changes.hugo_path != self.preferences.hugo_path;
            if hugo_changed {
                self.inspect_hugo(&changes.hugo_path)?;
            }
            let restart = hugo_changed
                || changes.preview_port != self.preferences.preview_port
                || changes.proxy != self.preferences.proxy;
            if changes.preview_port != self.preferences.preview_port {
                if let Some(port) = changes.preview_port {
                    bind_preview_port(port)?;
                }
            }
            self.storage.put(SETTINGS, "preferences", &changes)?;
            self.preferences = changes;
            if restart {
                self.stop_preview();
                self.approval = None;
            }
        }
        Ok(self.preferences.clone())
    }
    fn hugo_path(&self, configured: &str) -> Reply<PathBuf> {
        if configured.is_empty() {
            return Ok(self.bundled_hugo.clone());
        }
        let path = Path::new(configured);
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err("请选择 Hugo 可执行文件的完整 .exe 路径。".into());
        }
        Ok(path.to_path_buf())
    }
    pub fn inspect_hugo(&self, configured: &str) -> Reply<HugoInfo> {
        let path = self.hugo_path(configured.trim())?;
        if !path.is_file() {
            return Err("找不到 Hugo 可执行文件，请检查路径。".into());
        }
        let mut cmd = process(&path);
        cmd.arg("version");
        let version = run(cmd, "读取 Hugo 版本")?.trim().to_owned();
        if !version.to_ascii_lowercase().starts_with("hugo v") {
            return Err("这个文件未返回可识别的 Hugo 版本。".into());
        }
        Ok(HugoInfo {
            path: path_text(&path)?.into(),
            extended: version.contains("+extended"),
            version,
        })
    }
    pub fn detect_hugo(&self) -> Reply<HugoInfo> {
        if let Some(search) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&search) {
                let candidate = directory.join("hugo.exe");
                if directory.is_absolute() && candidate.is_file() {
                    if let Ok(info) = self.inspect_hugo(path_text(&candidate)?) {
                        return Ok(info);
                    }
                }
            }
        }
        self.inspect_hugo("")
    }
    fn git_command(&self, repo: Option<&Path>) -> Command {
        let mut command = process("git");
        if let Some(path) = repo {
            command.arg("-C").arg(path);
        }
        command.args(["-c", "core.autocrlf=false", "-c", "core.quotepath=false"]);
        command.env("GIT_TERMINAL_PROMPT", "0");
        if !self.preferences.proxy.is_empty() {
            command
                .arg("-c")
                .arg(format!("http.proxy={}", self.preferences.proxy));
        }
        command
    }
    fn git(&self, repo: Option<&Path>, args: &[&str]) -> Reply<String> {
        let mut command = self.git_command(repo);
        command.args(args);
        run(
            command,
            &format!("Git {}", args.first().copied().unwrap_or("")),
        )
    }
    fn revision(&self, repo: &Path) -> Reply<String> {
        Ok(self.git(Some(repo), &["rev-parse", "HEAD"])?.trim().into())
    }
    fn stage(&self, repo: &Path, files: &[String]) -> Reply<()> {
        let mut command = self.git_command(Some(repo));
        command
            .args(["--literal-pathspecs", "add", "--"])
            .args(files);
        run(command, "Git 添加文件")?;
        Ok(())
    }
    fn commit(&self, repo: &Path, title: &str) -> Reply<String> {
        let mut command = self.git_command(Some(repo));
        if self.git(Some(repo), &["var", "GIT_AUTHOR_IDENT"]).is_err() {
            command.args([
                "-c",
                "user.name=Hugo CMS",
                "-c",
                "user.email=hugo-cms@users.noreply.github.com",
            ]);
        }
        command.args(["commit", "-m", title]);
        run(command, "Git 提交")?;
        self.revision(repo)
    }

    fn source(&self) -> PathBuf {
        self.root.join("source")
    }

    fn sync(&self, refresh: bool) -> Reply<PathBuf> {
        let source = self.source();
        if !source.join(".git").is_dir() {
            let temp = tempfile::tempdir_in(&self.root).map_err(error)?;
            let clone = temp.path().join("site");
            self.site_git(
                None,
                &[
                    "clone",
                    "--branch",
                    self.branch(),
                    "--single-branch",
                    "--recurse-submodules",
                    &self.blog_remote,
                    path_text(&clone)?,
                ],
            )?;
            fs::rename(clone, &source).map_err(error)?;
        } else if refresh {
            self.site_git(
                Some(&source),
                &["pull", "--ff-only", "origin", self.branch()],
            )?;
            self.site_git(
                Some(&source),
                &["submodule", "update", "--init", "--recursive"],
            )?;
        }
        Ok(source)
    }
    pub fn library(&mut self, sync: bool) -> Reply<Library> {
        let warning = self.sync(sync).err();
        let drafts = self.storage.list(DRAFTS)?;
        let mut articles = Vec::new();
        let source = self.source();
        let posts = safe_join(&source, self.posts_dir())?;
        if posts.is_dir() {
            for entry in WalkDir::new(posts).follow_links(false) {
                let entry = entry.map_err(error)?;
                if !entry.file_type().is_file()
                    || entry.path().extension().is_none_or(|ext| ext != "md")
                    || is_section_index(entry.path())
                {
                    continue;
                }
                let path = path_text(entry.path().strip_prefix(&source).map_err(error)?)?
                    .replace('\\', "/");
                articles.push(self.read_article(&source, &path)?);
            }
        }
        Ok(Library {
            articles,
            drafts,
            warning,
        })
    }
    fn read_article(&self, source: &Path, path: &str) -> Reply<Article> {
        let bytes = fs::read(article_path(source, path, self.posts_dir())?).map_err(error)?;
        Ok(Article {
            path: path.into(),
            hash: hash(&bytes),
            markdown: String::from_utf8(bytes).map_err(error)?,
        })
    }
    fn drafts_dir(&self) -> PathBuf {
        self.root.join("drafts")
    }
    fn draft_dir(&self, draft: &Draft) -> PathBuf {
        self.drafts_dir().join(&draft.id)
    }
    fn draft_image(&self, draft: &Draft, image: &Picture) -> PathBuf {
        self.draft_dir(draft)
            .join("images")
            .join(format!("{}.png", image.id))
    }
    pub fn save(&self, draft: &Draft) -> Reply<()> {
        validate(draft, self.posts_dir())?;
        article_path(&self.source(), &draft.path, self.posts_dir())?;
        self.storage.put(DRAFTS, &draft.id, draft)
    }
    pub fn clipboard_picture(&self, draft: &Draft, hosted: bool) -> Reply<Picture> {
        validate(draft, self.posts_dir())?;
        let data = arboard::Clipboard::new()
            .map_err(error)?
            .get_image()
            .map_err(|_| "剪贴板里没有图片。")?;
        if data.width == 0
            || data.height == 0
            || data.width.saturating_mul(data.height) > 24_000_000
        {
            return Err("截图尺寸超出支持范围。".into());
        }
        let rgba = image::RgbaImage::from_raw(
            data.width as u32,
            data.height as u32,
            data.bytes.into_owned(),
        )
        .ok_or("截图数据无效。")?;
        let mut bytes = io::Cursor::new(Vec::new());
        rgba.write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(error)?;
        self.store_picture(draft, hosted, &bytes.into_inner())
    }
    pub fn import_picture(&self, draft: &Draft, hosted: bool, png: Vec<u8>) -> Reply<Picture> {
        validate(draft, self.posts_dir())?;
        if png.len() > 24 * 1024 * 1024 {
            return Err("图片超过 24 MB。".into());
        }
        let mut decoder =
            image::ImageReader::with_format(io::Cursor::new(&png), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(100 * 1024 * 1024);
        decoder.limits(limits);
        decoder.decode().map_err(|_| "图片格式无效或尺寸过大。")?;
        self.store_picture(draft, hosted, &png)
    }
    fn store_picture(&self, draft: &Draft, hosted: bool, png: &[u8]) -> Reply<Picture> {
        let hosted = hosted && self.site.as_ref().is_none_or(|site| site.hosted_images);
        if draft.images.len() >= 250 {
            return Err("每篇草稿最多保存 250 张图片。".into());
        }
        let id = Uuid::new_v4().to_string();
        let picture = Picture {
            path: picture_path(draft, &id),
            id,
            hosted,
        };
        atomic_write(&self.draft_image(draft, &picture), png)?;
        Ok(picture)
    }
    pub fn discard(&mut self, id: &str) -> Reply<()> {
        if !uuid(id) {
            return Err("草稿 ID 无效。".into());
        }
        let from = safe_join(&self.root, &format!("drafts/{id}"))?;
        self.storage.remove(DRAFTS, id)?;
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.id == id)
        {
            self.stop_preview();
        }
        // Draft records live in SQLite. An open editor or Explorer window can
        // prevent Windows from moving the draft directory. Keep those files in
        // place when archiving is unavailable; the draft is already discarded.
        if from.is_dir() {
            let _ = self.archive_draft_files(&from);
        }
        Ok(())
    }
    fn archive_draft_files(&self, from: &Path) -> Reply<()> {
        let to = safe_join(&self.root, &format!("discarded-drafts/{}", Uuid::new_v4()))?;
        fs::create_dir_all(to.parent().expect("discard directory")).map_err(error)?;
        fs::rename(from, to).map_err(error)
    }
    pub fn duplicate(&self, original: &Draft, mut copy: Draft) -> Reply<Draft> {
        validate(original, self.posts_dir())?;
        validate(&copy, self.posts_dir())?;
        if copy.original_path.is_some()
            || self.draft_dir(&copy).exists()
            || self.storage.get::<Draft>(DRAFTS, &copy.id)?.is_some()
        {
            return Err("新草稿已存在。".into());
        }
        copy.bundle_origin = original.bundle_origin.clone().or_else(|| {
            original
                .original_path
                .as_ref()
                .filter(|path| path.ends_with("/index.md"))
                .cloned()
        });
        copy.images.clear();
        for picture in active_images(original) {
            let mut new_image = picture.clone();
            new_image.path = picture_path(&copy, &picture.id);
            copy.markdown = copy.markdown.replace(&picture.path, &new_image.path);
            let data = fs::read(self.draft_image(original, picture)).map_err(error)?;
            atomic_write(&self.draft_image(&copy, &new_image), &data)?;
            copy.images.push(new_image);
        }
        self.save(&copy)?;
        Ok(copy)
    }
    fn article_directory(&self, path: Option<&str>, draft_id: Option<&str>) -> Reply<PathBuf> {
        match (path, draft_id) {
            (Some(path), None) => {
                let article = article_path(&self.source(), path, self.posts_dir())?;
                if !article.is_file() {
                    return Err("找不到该文章。请先同步 GitHub。".into());
                }
                Ok(article.parent().expect("article parent").to_path_buf())
            }
            (None, Some(id)) if uuid(id) => {
                if self.storage.get::<Draft>(DRAFTS, id)?.is_none() {
                    return Err("找不到该草稿。".into());
                }
                let directory = safe_join(&self.root, &format!("drafts/{id}"))?;
                fs::create_dir_all(&directory).map_err(error)?;
                Ok(directory)
            }
            _ => Err("无效的文章目录。".into()),
        }
    }
    pub fn open_article_directory(&self, path: Option<&str>, draft_id: Option<&str>) -> Reply<()> {
        open_local_path(&self.article_directory(path, draft_id)?)
    }
    pub fn copy_article_link(&self, path: &str) -> Reply<String> {
        let source = self.sync(false)?;
        if !article_path(&source, path, self.posts_dir())?.is_file() {
            return Err("找不到该文章。".into());
        }
        let url = self
            .permalink(&source, &source, path, self.website())?
            .to_string();
        arboard::Clipboard::new()
            .map_err(error)?
            .set_text(&url)
            .map_err(error)?;
        Ok(url)
    }
    fn stage_bundle(&self, checkout: &Path, draft: &Draft) -> Reply<HashSet<String>> {
        let mut files = HashSet::new();
        let Some(origin) = &draft.bundle_origin else {
            return Ok(files);
        };
        if !origin.ends_with("/index.md") || draft.original_path.is_some() {
            return Err("文章资源目录无效。".into());
        }
        let source = article_path(checkout, origin, self.posts_dir())?;
        if !source.is_file() {
            return Err("找不到被复制文章的资源目录。".into());
        }
        let base = source.parent().expect("article bundle");
        let (dest, _) = draft.path.rsplit_once('/').expect("validated article path");
        for entry in WalkDir::new(base).follow_links(false) {
            let entry = entry.map_err(error)?;
            if !entry.file_type().is_file() || entry.path() == source {
                continue;
            }
            let rel =
                path_text(entry.path().strip_prefix(base).map_err(error)?)?.replace('\\', "/");
            let source_path = safe_join(base, &rel)?;
            let file = format!("{dest}/{rel}");
            let target = safe_join(checkout, &file)?;
            if !target.exists() {
                atomic_write(&target, &fs::read(source_path).map_err(error)?)?;
            }
            files.insert(file);
        }
        Ok(files)
    }

    fn stage_draft(
        &self,
        checkout: &Path,
        draft: &Draft,
        hosted: Option<&str>,
    ) -> Reply<HashSet<String>> {
        let mut files = self.stage_bundle(checkout, draft)?;
        let mut markdown = draft.markdown.clone();
        for picture in active_images(draft) {
            if let Some(revision) = hosted.filter(|_| picture.hosted) {
                markdown = markdown.replace(&picture.path, &hosted_url(revision, draft, picture));
                continue;
            }
            let mut file = local_asset(draft, picture);
            if picture.path.starts_with('/') && !self.root_dir().is_empty() {
                file = format!("{}/{file}", self.root_dir());
            }
            let target = safe_join(checkout, &file)?;
            let bytes = fs::read(self.draft_image(draft, picture)).map_err(error)?;
            if !target.exists() {
                atomic_write(&target, &bytes)?;
            }
            files.insert(file);
        }
        atomic_write(
            &article_path(checkout, &draft.path, self.posts_dir())?,
            crlf(&markdown).as_bytes(),
        )?;
        Ok(files)
    }
    fn checkout(&self, source: &Path, destination: &Path) -> Reply<()> {
        self.git(
            None,
            &[
                "clone",
                "--quiet",
                "--branch",
                self.branch(),
                "--single-branch",
                "--no-hardlinks",
                path_text(source)?,
                path_text(destination)?,
            ],
        )?;
        Ok(())
    }
    fn hugo(&self, source: &Path, checkout: &Path) -> Reply<Command> {
        let hugo = self.hugo_path(&self.preferences.hugo_path)?;
        if !hugo.is_file() {
            return Err("找不到 Hugo，请在设置中检查可执行文件路径。".into());
        }
        let mut cmd = process(hugo);
        let root = self.site_root(checkout)?;
        cmd.current_dir(&root).arg("--source").arg(&root);
        cmd.arg("--themesDir")
            .arg(safe_join(&self.site_root(source)?, self.themes_dir())?);
        cmd.env("HUGO_CACHEDIR", self.root.join("hugo-cache"));
        if !self.preferences.proxy.is_empty() {
            cmd.env("HTTP_PROXY", &self.preferences.proxy)
                .env("HTTPS_PROXY", &self.preferences.proxy)
                .env("NO_PROXY", "localhost,127.0.0.1");
        }
        Ok(cmd)
    }
    fn permalink(&self, source: &Path, checkout: &Path, article: &str, origin: &str) -> Reply<Url> {
        let mut cmd = self.hugo(source, checkout)?;
        cmd.args(["list", "all"]).env("HUGO_BASEURL", origin);
        let list = run(cmd, "读取 Hugo 文章地址")?;
        let mut csv = csv::ReaderBuilder::new().from_reader(list.as_bytes());
        let header = csv.headers().map_err(error)?.clone();
        let path_column = header
            .iter()
            .position(|field| field == "path")
            .ok_or("Hugo 输出没有 path。")?;
        let url_column = header
            .iter()
            .position(|field| field == "permalink")
            .ok_or("Hugo 输出没有 permalink。")?;
        let relative = self.relative_article(article);
        let short = relative
            .strip_prefix(self.content_dir())
            .and_then(|p| p.strip_prefix('/'));
        for record in csv.records() {
            let record = record.map_err(error)?;
            let path = record.get(path_column).unwrap_or("").replace('\\', "/");
            if path == relative || short == Some(path.as_str()) {
                let url = Url::parse(record.get(url_column).unwrap_or("")).map_err(error)?;
                if url.origin() != Url::parse(origin).map_err(error)?.origin() {
                    return Err("Hugo 返回了不匹配的站点地址。".into());
                }
                return Ok(url);
            }
        }
        Err("Hugo 没有找到文章，请检查 YAML 和文章路径。".into())
    }
    pub fn open_preview(&mut self, url: &str) -> Reply<()> {
        let preview = self.preview.as_mut().ok_or("Hugo 预览尚未就绪。")?;
        if preview.child.try_wait().map_err(error)?.is_some() {
            return Err("Hugo 预览已停止，请刷新页面。".into());
        }
        let target = validate_preview_target(&preview.origin, url)?;
        open_in_browser(&target)
    }
    pub fn stop_preview(&mut self) {
        self.preview = None;
    }
    pub fn preview(&mut self, draft: &Draft) -> Reply<String> {
        validate(draft, self.posts_dir())?;
        let source = self.sync(false)?;
        let revision = self.revision(&source)?;
        if let Some(preview) = &self.preview {
            if preview.id == draft.id && preview.path == draft.path && preview.revision == revision
            {
                let checkout = preview.dir.path().join("site");
                let origin = preview.origin.clone();
                let previous = preview.images.clone();
                let files = self.stage_draft(&checkout, draft, None)?;
                for gone in previous.difference(&files) {
                    let path = safe_join(&checkout, gone)?;
                    if path.is_file() {
                        fs::remove_file(path).map_err(error)?;
                    }
                }
                let themes = if preview.updated_modules { &checkout } else { &source };
                let permalink = self.permalink(themes, &checkout, &draft.path, &origin)?;
                let preview = self.preview.as_mut().expect("active preview");
                preview.images = files;
                if preview.child.try_wait().map_err(error)?.is_none() {
                    return Ok(permalink.into());
                }
            }
        }
        self.stop_preview();
        let dir = tempfile::tempdir_in(&self.root).map_err(error)?;
        let checkout = dir.path().join("site");
        self.checkout(&source, &checkout)?;
        let updated_modules = self.apply_preview_modules(&source, &revision, &checkout)?;
        let themes = if updated_modules { &checkout } else { &source };
        let images = self.stage_draft(&checkout, draft, None)?;
        let socket = bind_preview_port(self.preferences.preview_port.unwrap_or_default())?;
        let address = socket.local_addr().map_err(error)?;
        let origin = format!("http://{address}/");
        let permalink = self.permalink(themes, &checkout, &draft.path, &origin)?;
        let log = dir.path().join("hugo.log");
        let output = fs::File::create(&log).map_err(error)?;
        let stderr = output.try_clone().map_err(error)?;
        let mut cmd = self.hugo(themes, &checkout)?;
        cmd.args([
            "server",
            "-D",
            "--bind",
            "127.0.0.1",
            "--port",
            &address.port().to_string(),
            "--baseURL",
            &origin,
            "--appendPort=false",
            "--disableFastRender",
            "--noHTTPCache",
        ]);
        drop(socket);
        let child = cmd.stdout(output).stderr(stderr).spawn().map_err(error)?;
        let mut preview = Preview {
            child,
            dir,
            log,
            origin,
            images,
            id: draft.id.clone(),
            path: draft.path.clone(),
            revision,
            updated_modules,
        };
        let deadline = Instant::now() + Duration::from_secs(16);
        loop {
            if preview.child.try_wait().map_err(error)?.is_some() || Instant::now() > deadline {
                let logs = fs::read_to_string(&preview.log).unwrap_or_default();
                return Err(format!("Hugo 预览服务未启动。\n{logs}"));
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(80)).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.preview = Some(preview);
        Ok(permalink.into())
    }
    fn verify_base(&self, source: &Path, draft: &Draft) -> Reply<()> {
        let file = article_path(source, &draft.path, self.posts_dir())?;
        match (&draft.original_hash, fs::read(file)) {
            (Some(base), Ok(bytes)) if hash(&bytes) == *base => Ok(()),
            (None, Err(err)) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err("远端文章已更新，或该 Slug 已存在。草稿已保留。".into()),
        }
    }
    fn operation(draft: &Draft) -> Reply<String> {
        Ok(hash(&serde_json::to_vec(draft).map_err(error)?))
    }

    fn already_pushed(&self, source: &Path, commit: Option<&str>) -> bool {
        commit.is_some_and(|revision| {
            self.git(
                Some(source),
                &["merge-base", "--is-ancestor", revision, "HEAD"],
            )
            .is_ok()
        })
    }
    fn check_url(&self, source: &Path, checkout: &Path, draft: &Draft) -> Reply<Url> {
        let url = self.permalink(source, checkout, &draft.path, self.website())?;
        if draft.original_path.is_none() {
            return Ok(url);
        }
        let previous = self.permalink(source, source, &draft.path, self.website())?;
        if previous.path() == url.path() {
            return Ok(url);
        }
        let target = article_path(checkout, &draft.path, self.posts_dir())?;
        let markdown = fs::read_to_string(&target).map_err(error)?;
        let with_url = preserve_url(&markdown, &previous)?;
        atomic_write(&target, with_url.as_bytes())?;
        let confirmed = self.permalink(source, checkout, &draft.path, self.website())?;
        if confirmed.path() != previous.path() {
            return Err("无法保留原文章地址。请检查文章的 Slug 和 URL。".into());
        }
        Ok(confirmed)
    }
    pub fn prepare(&mut self, draft: &Draft, release: &Draft) -> Reply<Summary> {
        self.save(draft)?;
        let draft = release;
        validate(draft, self.posts_dir())?;
        let source = self.sync(true)?;
        let revision = self.revision(&source)?;
        let key = Self::operation(draft)?;

        let recovered = {
            let journal = self
                .storage
                .get::<Journal>(OPERATIONS, &key)?
                .unwrap_or_default();
            self.already_pushed(&source, journal.commit.as_deref())
        };
        let mut files = vec![draft.path.clone()];
        let mut hosted_files = Vec::new();
        for picture in active_images(draft) {
            if picture.hosted {
                hosted_files.push(hosted_asset(draft, picture));
            } else {
                files.push(local_asset(draft, picture));
            }
        }
        let (url, diff) = if recovered {
            (
                self.permalink(&source, &source, &draft.path, self.website())?,
                "此提交已在 GitHub，可以恢复本地发布状态。".to_owned(),
            )
        } else {
            self.verify_base(&source, draft)?;
            let temp = tempfile::tempdir_in(&self.root).map_err(error)?;
            let checkout = temp.path().join("site");
            self.checkout(&source, &checkout)?;
            let assets = self.stage_draft(&checkout, draft, None)?;
            files.extend(assets.into_iter().filter(|file| {
                !active_images(draft)
                    .any(|image| image.hosted && local_asset(draft, image) == *file)
            }));
            files.sort();
            files.dedup();
            let url = self.check_url(&source, &checkout, draft)?;
            self.stage(&checkout, &files)?;
            let diff = self.git(
                Some(&checkout),
                &[
                    "--literal-pathspecs",
                    "diff",
                    "--cached",
                    "--no-color",
                    "--",
                    &draft.path,
                ],
            )?;
            (url, diff)
        };
        let approval = Uuid::new_v4().to_string();
        self.approval = Some((approval.clone(), key, revision));
        Ok(Summary {
            url: url.into(),
            files,
            hosted_files,
            diff,
            approval,
        })
    }

    pub fn publish(&mut self, draft: &Draft, approval: &str) -> Reply<PublishResult> {
        validate(draft, self.posts_dir())?;
        let key = Self::operation(draft)?;
        let (token, approved_key, revision) = self.approval.take().ok_or("请先查看发布摘要。")?;
        if token != approval || approved_key != key {
            return Err("文章在确认后有变化，请重新核对摘要。".into());
        }
        let source = self.sync(true)?;

        let journal = {
            let mut journal = self
                .storage
                .get::<Journal>(OPERATIONS, &key)?
                .unwrap_or_default();
            if self.already_pushed(&source, journal.commit.as_deref()) {
                let url = self.permalink(&source, &source, &draft.path, self.website())?;
                let published = Published {
                    path: url.path().into(),
                    commit: journal.commit.take().ok_or("找不到发布提交。")?,
                };
                return self.finish(draft, published, self.read_article(&source, &draft.path)?);
            }
            journal
        };
        if self.revision(&source)? != revision {
            return Err("GitHub main 在确认后有更新。请重新核对发布摘要。".into());
        }
        self.verify_base(&source, draft)?;
        let temp = tempfile::tempdir_in(&self.root).map_err(error)?;
        let checkout = temp.path().join("site");
        self.checkout(&source, &checkout)?;
        let _assets = self.stage_draft(&checkout, draft, None)?;
        let url = self.check_url(&source, &checkout, draft)?;
        self.build(&source, &checkout, temp.path(), &url)?;

        {
            let mut journal = journal;
            let hosted: Vec<_> = active_images(draft)
                .filter(|picture| picture.hosted)
                .collect();
            if !hosted.is_empty() {
                let revision =
                    self.publish_images(draft, &hosted, temp.path(), &mut journal, &key)?;
                for picture in &hosted {
                    fs::remove_file(safe_join(&checkout, &local_asset(draft, picture))?)
                        .map_err(error)?;
                }
                self.stage_draft(&checkout, draft, Some(&revision))?;
                self.check_url(&source, &checkout, draft)?;
                self.build(&source, &checkout, temp.path(), &url)?;
            }
            let mut files = vec![draft.path.clone()];
            files.extend(_assets.into_iter().filter(|file| {
                !hosted
                    .iter()
                    .any(|image| local_asset(draft, image) == *file)
            }));
            files.sort();
            self.stage(&checkout, &files)?;
            if self
                .git(Some(&checkout), &["diff", "--cached", "--name-only"])?
                .trim()
                .is_empty()
            {
                return Err("这篇文章没有可发布的改动。".into());
            }
            let article = self.read_article(&checkout, &draft.path)?;
            let commit = self.commit(&checkout, &format!("Publish {}", draft.path))?;
            journal.commit = Some(commit.clone());
            self.storage.put(OPERATIONS, &key, &journal)?;
            self.site_git(
                Some(&checkout),
                &[
                    "push",
                    &self.blog_remote,
                    &format!("HEAD:refs/heads/{}", self.branch()),
                ],
            )?;
            let published = Published {
                path: url.path().into(),
                commit,
            };
            journal.published = Some(published.clone());
            self.storage.put(OPERATIONS, &key, &journal)?;
            self.finish(draft, published, article)
        }
    }

    fn finish(
        &mut self,
        draft: &Draft,
        published: Published,
        article: Article,
    ) -> Reply<PublishResult> {
        self.storage.remove(DRAFTS, &draft.id)?;
        self.stop_preview();
        Ok(PublishResult { published, article })
    }
    pub fn delete_article(&mut self, path: &str, expected_hash: &str) -> Reply<String> {
        article_path(&self.source(), path, self.posts_dir())?;
        let source = self.sync(true)?;
        let key = format!(
            "delete-{}",
            hash(&serde_json::to_vec(&(path, expected_hash)).map_err(error)?)
        );
        let previous: Option<String> = self.storage.get(OPERATIONS, &key)?;
        let file = article_path(&source, path, self.posts_dir())?;
        match fs::read(file) {
            Ok(bytes) if hash(&bytes) == expected_hash => {}
            Err(err)
                if err.kind() == io::ErrorKind::NotFound
                    && self.already_pushed(&source, previous.as_deref()) =>
            {
                return previous.ok_or_else(|| "找不到删除提交。".into());
            }
            _ => return Err("GitHub 上的文章已变化或已移除。请刷新列表并重新确认。".into()),
        }

        let temp = tempfile::tempdir_in(&self.root).map_err(error)?;
        let checkout = temp.path().join("site");
        self.checkout(&source, &checkout)?;
        if self.read_article(&checkout, path)?.hash != expected_hash {
            return Err("文章有尚未同步的文件改动，请刷新列表后再试。".into());
        }
        self.git(Some(&checkout), &["--literal-pathspecs", "rm", "--", path])?;
        let mut cmd = self.hugo(&source, &checkout)?;
        cmd.arg("--destination")
            .arg(temp.path().join("public"))
            .env("HUGO_BASEURL", self.website());
        run(cmd, "删除后的 Hugo 正式构建")?;
        let commit = self.commit(&checkout, &format!("Delete {path}"))?;
        self.storage.put(OPERATIONS, &key, &commit)?;
        self.site_git(
            Some(&checkout),
            &[
                "push",
                &self.blog_remote,
                &format!("HEAD:refs/heads/{}", self.branch()),
            ],
        )?;
        self.stop_preview();
        self.approval = None;
        Ok(commit)
    }

    fn build(&self, source: &Path, checkout: &Path, temp: &Path, url: &Url) -> Reply<()> {
        let output = tempfile::tempdir_in(temp).map_err(error)?;
        let mut cmd = self.hugo(source, checkout)?;
        cmd.arg("--destination")
            .arg(output.path())
            .env("HUGO_BASEURL", self.website());
        run(cmd, "Hugo 正式构建")?;
        let base = Url::parse(self.website()).map_err(error)?;
        let relative = url
            .path()
            .strip_prefix(base.path())
            .unwrap_or_else(|| url.path().trim_start_matches('/'));
        let article = percent_decode(relative.trim_matches('/'))?;
        let html = if article.is_empty() {
            "index.html".into()
        } else if article.ends_with(".html") {
            article
        } else {
            format!("{article}/index.html")
        };
        if !safe_join(output.path(), &html)?.is_file() {
            return Err("正式构建没有生成该文章。请取消草稿状态并检查发布日期。".into());
        }
        Ok(())
    }

    fn publish_images(
        &self,
        draft: &Draft,
        pictures: &[&Picture],
        temp: &Path,
        journal: &mut Journal,
        journal_key: &str,
    ) -> Reply<String> {
        let revision = self.push_images(draft, pictures, temp, journal, journal_key)?;
        let mut client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(8));
        if !self.preferences.proxy.is_empty() {
            client = client.proxy(reqwest::Proxy::all(&self.preferences.proxy).map_err(error)?);
        }
        let client = client.build().map_err(error)?;
        for picture in pictures {
            let url = hosted_url(&revision, draft, picture);
            let mut found = false;
            for attempt in 0..3 {
                if client.head(&url).send().is_ok_and(|response| {
                    response.status().is_success()
                        && response
                            .headers()
                            .get("content-type")
                            .is_some_and(|value| value.as_bytes().starts_with(b"image/"))
                }) {
                    found = true;
                    break;
                }
                if attempt < 2 {
                    thread::sleep(Duration::from_secs(2));
                }
            }
            if !found {
                return Err(
                    "图片已推送到图床，但公开链接尚不可访问。可稍后重试，或检查代理设置。".into(),
                );
            }
        }
        Ok(revision)
    }

    fn push_images(
        &self,
        draft: &Draft,
        pictures: &[&Picture],
        temp: &Path,
        journal: &mut Journal,
        journal_key: &str,
    ) -> Reply<String> {
        let repo = temp.join("images");
        self.git(
            None,
            &[
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--branch",
                "master",
                "--single-branch",
                &self.hosting_remote,
                path_text(&repo)?,
            ],
        )?;
        let mut changed = Vec::new();
        for picture in pictures {
            let path = hosted_asset(draft, picture);
            let destination = safe_join(&repo, &path)?;
            let bytes = fs::read(self.draft_image(draft, picture)).map_err(error)?;
            if destination.exists() {
                if fs::read(destination).map_err(error)? != bytes {
                    return Err("图床已有同名但内容不同的图片。".into());
                }
            } else {
                atomic_write(&destination, &bytes)?;
                changed.push(path);
            }
        }
        let revision = if changed.is_empty() {
            journal
                .hosted_commit
                .clone()
                .unwrap_or(self.revision(&repo)?)
        } else {
            self.stage(&repo, &changed)?;
            let revision = self.commit(&repo, "Add Hugo article images")?;
            self.git(Some(&repo), &["push", "origin", "HEAD:refs/heads/master"])?;
            revision
        };
        journal.hosted_commit = Some(revision.clone());
        self.storage.put(OPERATIONS, journal_key, journal)?;
        Ok(revision)
    }
}

fn preserve_url(markdown: &str, prior: &Url) -> Reply<String> {
    let header = markdown
        .strip_prefix("---\r\n")
        .ok_or("文章缺少 YAML 元数据。")?;
    let end = header
        .find("\r\n---")
        .ok_or("文章缺少 YAML 元数据结束标记。")?;
    if header[..end].lines().any(|line| {
        line.starts_with("url:") || line.starts_with("\"url\":") || line.starts_with("'url':")
    }) {
        return Err("请保留原 YAML 中的 URL。".into());
    }
    let path = percent_decode(prior.path())?;
    let scalar = serde_json::to_string(&path).map_err(error)?;
    Ok(format!("---\r\nurl: {scalar}\r\n{header}"))
}

fn percent_decode(text: &str) -> Reply<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%' {
            let hex = bytes
                .get(cursor + 1..cursor + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .ok_or("URL 转义无效。")?;
            decoded.push(u8::from_str_radix(hex, 16).map_err(error)?);
            cursor += 3;
        } else {
            decoded.push(bytes[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(decoded).map_err(error)
}

impl Workspace {
    pub fn open_page(&self, page: &str, path: Option<&str>) -> Reply<()> {
        let url = match page {
            "about_github" => Url::parse("https://github.com/hengxin007").map_err(error)?,
            "github" => Url::parse(
                self.site
                    .as_ref()
                    .map_or(BLOG, |site| site.remote.as_str())
                    .trim_end_matches(".git"),
            )
            .map_err(error)?,
            "cloudflare" => Url::parse(&self.cloudflare_dashboard()).map_err(error)?,
            "article" => {
                let path = path
                    .filter(|path| path.starts_with('/') && !path.starts_with("//"))
                    .ok_or("无效的文章路径。")?;
                let origin = Url::parse(self.website()).map_err(error)?;
                let target = origin.join(path).map_err(error)?;
                if target.origin() != origin.origin() {
                    return Err("无效的文章地址。".into());
                }
                target
            }
            _ => return Err("无效的页面。".into()),
        };
        open_in_browser(&url)
    }
}

fn validate_preview_target(origin: &str, url: &str) -> Reply<Url> {
    let target = Url::parse(url).map_err(error)?;
    let active = Url::parse(origin).map_err(error)?;
    if target.scheme() != "http"
        || target.host_str() != Some("127.0.0.1")
        || target.origin() != active.origin()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err("只能打开当前 Hugo 预览服务中的页面。".into());
    }
    Ok(target)
}

#[cfg(windows)]
fn open_local_path(path: &Path) -> Reply<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        ) as isize
    };
    match result {
        0..=32 => Err(format!("无法打开本地路径（Windows Shell 错误 {result}）。")),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
fn open_local_path(_: &Path) -> Reply<()> {
    Err("仅支持在 Windows 中打开本地路径。".into())
}

fn open_in_browser(url: &Url) -> Reply<()> {
    process("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url.as_str())
        .spawn()
        .map(|_| ())
        .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_only_opens_the_active_preview_origin() {
        let origin = "http://127.0.0.1:13131/";
        let page = "http://127.0.0.1:13131/p/test/?view=compact#heading";
        assert_eq!(
            validate_preview_target(origin, page).unwrap().as_str(),
            page
        );
        for outside_page in [
            "http://127.0.0.1:13132/",
            "https://127.0.0.1:13131/",
            "file:///C:/Windows/",
            "https://example.com/",
            "http://name:pass@127.0.0.1:13131/",
        ] {
            assert!(validate_preview_target(origin, outside_page).is_err());
        }
    }

    #[test]
    fn validates_windows_paths_and_preserves_encoding() {
        let temp = tempfile::tempdir().unwrap();
        assert!(safe_join(temp.path(), "content/post/../lost.md").is_err());
        assert!(safe_join(temp.path(), "content/post/:stream.md").is_err());
        assert_eq!(crlf("\u{feff}中文\nline\r\n"), "中文\r\nline\r\n");
        assert_eq!(
            percent_decode("p/%E6%B5%8B%E8%AF%95/index.html").unwrap(),
            "p/测试/index.html"
        );
    }

    #[test]
    fn article_directory_uses_the_repository_or_saved_draft_location() {
        let fixture = tempfile::tempdir().unwrap();
        let ws = Workspace::new(fixture.path().to_path_buf(), PathBuf::new()).unwrap();
        let stored = ws.source().join("content/post/published/index.md");
        atomic_write(&stored, b"published article").unwrap();
        assert_eq!(
            ws.article_directory(Some("content/post/published/index.md"), None)
                .unwrap(),
            stored.parent().unwrap()
        );
        assert!(ws
            .article_directory(Some("content/post/../../../outside.md"), None)
            .is_err());

        let draft = Draft {
            id: Uuid::new_v4().to_string(),
            path: "content/post/unpublished/index.md".into(),
            original_path: None,
            original_hash: None,
            bundle_origin: None,
            markdown: "unpublished draft".into(),
            images: vec![],
        };
        assert!(ws.article_directory(None, Some(&draft.id)).is_err());
        ws.save(&draft).unwrap();
        let directory = ws.article_directory(None, Some(&draft.id)).unwrap();
        assert_eq!(directory, ws.draft_dir(&draft));
        assert!(directory.is_dir());
        assert!(!ws.source().join(&draft.path).exists());
    }

    #[cfg(windows)]
    #[test]
    fn discards_metadata_edits_when_windows_locks_the_draft_directory() {
        use std::os::windows::fs::OpenOptionsExt;

        let fixture = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(fixture.path().to_path_buf(), PathBuf::new()).unwrap();
        let path = "content/post/published/index.md";
        let stored = ws.source().join(path);
        let original = b"---\ncategories: [existing]\n---\narticle body\n";
        atomic_write(&stored, original).unwrap();
        let draft = Draft {
            id: Uuid::new_v4().to_string(),
            path: path.into(),
            original_path: Some(path.into()),
            original_hash: Some(hash(original)),
            bundle_origin: None,
            markdown: "---\ncategories: []\n---\narticle body\n".into(),
            images: vec![],
        };
        ws.save(&draft).unwrap();
        let directory = ws.draft_dir(&draft);
        fs::create_dir_all(&directory).unwrap();
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&directory)
            .unwrap();
        assert!(ws.archive_draft_files(&directory).is_err());
        ws.discard(&draft.id).unwrap();
        assert!(ws.storage.list::<Draft>(DRAFTS).unwrap().is_empty());
        assert_eq!(fs::read(stored).unwrap(), original);
        drop(handle);
        assert!(directory.is_dir());
        ws.discard(&draft.id).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn publishes_one_post_to_an_isolated_git_remote() {
        let fixture = tempfile::tempdir().unwrap();
        let hugo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries/hugo-x86_64-pc-windows-msvc.exe");
        let mut ws = Workspace::new(fixture.path().join("cms"), hugo).unwrap();
        let upstream = fixture.path().join("upstream.git");
        let source = fixture.path().join("fixture");
        ws.git(
            None,
            &[
                "init",
                "--bare",
                "-b",
                "main",
                path_text(&upstream).unwrap(),
            ],
        )
        .unwrap();
        ws.git(None, &["init", "-b", "main", path_text(&source).unwrap()])
            .unwrap();
        atomic_write(
            &source.join("hugo.yaml"),
            b"baseURL: https://544688.xyz/\npermalinks:\n  post: /p/:slug/\n",
        )
        .unwrap();
        atomic_write(
            &source.join("layouts/_default/single.html"),
            b"<!DOCTYPE html><main>{{ .Content }}</main>\n",
        )
        .unwrap();
        ws.stage(
            &source,
            &["hugo.yaml".into(), "layouts/_default/single.html".into()],
        )
        .unwrap();
        ws.commit(&source, "fixture").unwrap();
        ws.git(
            Some(&source),
            &[
                "push",
                path_text(&upstream).unwrap(),
                "HEAD:refs/heads/main",
            ],
        )
        .unwrap();
        ws.blog_remote = path_text(&upstream).unwrap().into();
        let draft = |slug: &str| Draft {
            id: Uuid::new_v4().to_string(),
            path: format!("content/post/{slug}/index.md"),
            original_path: None,
            original_hash: None,
            bundle_origin: None,
            markdown: format!("---\ntitle: 测试\nslug: {slug}\ndraft: false\n---\n中文内容\n"),
            images: vec![],
        };
        let mut first = draft("first");
        let second = draft("second");
        let picture_id = Uuid::new_v4().to_string();
        let picture = Picture {
            path: picture_path(&first, &picture_id),
            id: picture_id,
            hosted: false,
        };
        first
            .markdown
            .push_str(&format!("![示例]({})\n", picture.path));
        let png = image::RgbaImage::new(2, 2);
        let mut encoded = io::Cursor::new(Vec::new());
        png.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        atomic_write(&ws.draft_image(&first, &picture), &encoded.into_inner()).unwrap();
        first.images.push(picture);
        ws.save(&second).unwrap();
        let preview = ws.preview(&first).unwrap();
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        assert!(http
            .get(&preview)
            .send()
            .unwrap()
            .text()
            .unwrap()
            .contains("中文内容"));
        let mut live_edit = first.clone();
        live_edit.markdown.push_str("\n预览自动更新\n");
        assert_eq!(ws.preview(&live_edit).unwrap(), preview);
        let until = Instant::now() + Duration::from_secs(6);
        loop {
            if http
                .get(&preview)
                .send()
                .unwrap()
                .text()
                .unwrap()
                .contains("预览自动更新")
            {
                break;
            }
            assert!(Instant::now() < until, "Hugo did not refresh the preview");
            thread::sleep(Duration::from_millis(150));
        }
        assert!(preview.ends_with("/p/first/"), "{preview}");
        let summary = ws.prepare(&first, &first).unwrap();
        assert!(summary.url.ends_with("/p/first/"));
        assert!(ws.publish(&second, &summary.approval).is_err());
        let summary = ws.prepare(&first, &first).unwrap();
        let published = ws.publish(&first, &summary.approval).unwrap();
        let paths = ws
            .git(
                Some(&upstream),
                &[
                    "ls-tree",
                    "-r",
                    "--name-only",
                    published.published.commit.as_str(),
                ],
            )
            .unwrap();
        assert!(paths.contains("content/post/first/index.md"));
        assert!(!paths.contains("content/post/second/index.md"));
        assert!(paths.contains(&local_asset(&first, &first.images[0])));
        let mut recovered = Workspace::new(ws.root.clone(), ws.bundled_hugo.clone()).unwrap();
        recovered.blog_remote = ws.blog_remote.clone();
        let library = recovered.library(true).unwrap();
        assert_eq!(library.drafts.len(), 1);
        assert_eq!(library.drafts[0].id, second.id);
        assert_eq!(library.articles.len(), 1);
        let mut excluded = second.clone();
        excluded.markdown = excluded.markdown.replace("draft: false", "draft: true");
        let excluded_summary = recovered.prepare(&excluded, &excluded).unwrap();
        assert!(recovered
            .publish(&excluded, &excluded_summary.approval)
            .unwrap_err()
            .contains("没有生成该文章"));
        assert!(recovered
            .storage
            .get::<Draft>(DRAFTS, &excluded.id)
            .unwrap()
            .is_some());
        let summary = recovered.prepare(&second, &second).unwrap();
        ws.git(
            Some(&source),
            &["pull", "--ff-only", path_text(&upstream).unwrap(), "main"],
        )
        .unwrap();
        atomic_write(&source.join("README.md"), b"updated upstream\r\n").unwrap();
        ws.stage(&source, &["README.md".into()]).unwrap();
        ws.commit(&source, "upstream update").unwrap();
        ws.git(
            Some(&source),
            &[
                "push",
                path_text(&upstream).unwrap(),
                "HEAD:refs/heads/main",
            ],
        )
        .unwrap();
        assert!(recovered
            .publish(&second, &summary.approval)
            .unwrap_err()
            .contains("确认后有更新"));
        assert!(recovered
            .storage
            .get::<Draft>(DRAFTS, &second.id)
            .unwrap()
            .is_some());
        let article = recovered.library(false).unwrap().articles.pop().unwrap();
        assert_eq!(published.article.path, article.path);
        assert_eq!(published.article.hash, article.hash);
        assert!(recovered
            .delete_article(&article.path, "stale hash")
            .is_err());
        assert!(recovered
            .delete_article("hugo.yaml", &article.hash)
            .is_err());
        let removed = recovered
            .delete_article(&article.path, &article.hash)
            .unwrap();
        assert_eq!(
            recovered
                .delete_article(&article.path, &article.hash)
                .unwrap(),
            removed
        );
        let status = recovered
            .git(
                Some(&upstream),
                &[
                    "diff-tree",
                    "--no-commit-id",
                    "--name-status",
                    "-r",
                    &removed,
                ],
            )
            .unwrap();
        assert_eq!(status.trim(), format!("D\t{}", article.path));
        let library = recovered.library(true).unwrap();
        assert!(library.articles.is_empty());
        assert_eq!(library.drafts[0].id, second.id);
        assert!(recovered
            .source()
            .join(local_asset(&first, &first.images[0]))
            .is_file());
    }

    #[test]
    fn reuses_hosted_image_commit_after_a_failed_release() {
        let fixture = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(fixture.path().join("cms"), PathBuf::new()).unwrap();
        let remote = fixture.path().join("images.git");
        let seed = fixture.path().join("seed");
        workspace
            .git(
                None,
                &[
                    "init",
                    "--bare",
                    "-b",
                    "master",
                    path_text(&remote).unwrap(),
                ],
            )
            .unwrap();
        workspace
            .git(None, &["init", "-b", "master", path_text(&seed).unwrap()])
            .unwrap();
        atomic_write(&seed.join("README.md"), b"fixture\r\n").unwrap();
        workspace.stage(&seed, &["README.md".into()]).unwrap();
        workspace.commit(&seed, "fixture").unwrap();
        workspace
            .git(
                Some(&seed),
                &[
                    "push",
                    path_text(&remote).unwrap(),
                    "HEAD:refs/heads/master",
                ],
            )
            .unwrap();
        workspace.hosting_remote = path_text(&remote).unwrap().into();
        let draft = Draft {
            id: Uuid::new_v4().to_string(),
            path: "content/post/example/index.md".into(),
            original_path: None,
            original_hash: None,
            bundle_origin: None,
            markdown: String::new(),
            images: vec![],
        };
        let id = Uuid::new_v4().to_string();
        let picture = Picture {
            path: picture_path(&draft, &id),
            id,
            hosted: true,
        };
        atomic_write(&workspace.draft_image(&draft, &picture), b"test bytes").unwrap();
        let journal_key = "test";
        let mut journal = Journal::default();
        let first_attempt = tempfile::tempdir_in(fixture.path()).unwrap();
        let first = workspace
            .push_images(
                &draft,
                &[&picture],
                first_attempt.path(),
                &mut journal,
                journal_key,
            )
            .unwrap();
        let second_attempt = tempfile::tempdir_in(fixture.path()).unwrap();
        let mut journal = workspace
            .storage
            .get::<Journal>(OPERATIONS, journal_key)
            .unwrap()
            .unwrap();
        let second = workspace
            .push_images(
                &draft,
                &[&picture],
                second_attempt.path(),
                &mut journal,
                journal_key,
            )
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            workspace
                .git(Some(&remote), &["rev-list", "--count", "master"])
                .unwrap()
                .trim(),
            "2"
        );
        assert!(hosted_url(&second, &draft, &picture).contains(&format!("@{first}/hugo-cms/")));
    }
}
