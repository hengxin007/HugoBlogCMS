use super::*;

use base64::Engine;
use reqwest::blocking::Client;
use serde_json::Value;

const WELCOME_ACKNOWLEDGED: &str = "welcome-acknowledged";
const MARKER: &str = "hugo-cms-deploy.json";
const PAGES_BUILD: &str = "hugo && printf '{\"commit\":\"%s\",\"branch\":\"%s\"}\\n' \"$CF_PAGES_COMMIT_SHA\" \"$CF_PAGES_BRANCH\" > public/hugo-cms-deploy.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteConfig {
    pub title: String,
    pub remote: String,
    pub branch: String,
    root_dir: String,
    content_dir: String,
    pub posts_dir: String,
    themes_dir: String,
    pub website: String,
    pages_project: String,
    pages_url: String,
    account: String,
    pub hosted_images: bool,
}

impl SiteConfig {
    fn legacy() -> Self {
        Self {
            title: "张稳妥的博客".into(),
            remote: BLOG.into(),
            branch: "main".into(),
            root_dir: String::new(),
            content_dir: "content".into(),
            posts_dir: "content/post".into(),
            themes_dir: "themes".into(),
            website: SITE.into(),
            pages_project: "blog".into(),
            pages_url: String::new(),
            account: String::new(),
            hosted_images: true,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Create,
    Import,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Input {
    title: String,
    remote: String,
    branch: String,
    root_dir: String,
    posts_dir: String,
    website: String,
    pages_project: String,
    pages_url: String,
    account: String,
    hugo_path: String,
    proxy: String,
    confirmed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wizard {
    id: String,
    route: Route,
    #[serde(default)]
    existing_site: bool,
    step: u8,
    input: Input,
    checkout_remote: String,
    checkout_branch: String,
    first_commit: String,
    hugo_version: String,
    theme: String,
    candidates: Vec<String>,
    articles: usize,
    unsupported: usize,
    sample: Option<String>,
    inspected: Option<SiteConfig>,
    checked: Option<SiteConfig>,
    deployment: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    site: Option<SiteConfig>,
    wizard: Option<Wizard>,
    show_welcome: bool,
    pages_build: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start(Route),
    Continue,
    Save,
    Back,
    Generate,
    Connect,
    Detect,
    Inspect,
    Push,
    Verify,
    Finish,
}

#[derive(Serialize)]
pub struct SetupPreview {
    home: String,
    article: Option<String>,
}

impl Workspace {
    pub(super) fn branch(&self) -> &str {
        self.site.as_ref().map_or("main", |site| &site.branch)
    }
    pub(super) fn root_dir(&self) -> &str {
        self.site.as_ref().map_or("", |site| &site.root_dir)
    }
    pub(super) fn posts_dir(&self) -> &str {
        self.site
            .as_ref()
            .map_or("content/post", |site| &site.posts_dir)
    }
    pub(super) fn content_dir(&self) -> &str {
        self.site
            .as_ref()
            .map_or("content", |site| &site.content_dir)
    }
    pub(super) fn themes_dir(&self) -> &str {
        self.site.as_ref().map_or("themes", |site| &site.themes_dir)
    }
    pub(super) fn website(&self) -> &str {
        self.site.as_ref().map_or(SITE, |site| &site.website)
    }
    pub(super) fn site_root(&self, checkout: &Path) -> Reply<PathBuf> {
        subdir(checkout, self.root_dir())
    }
    pub(super) fn relative_article<'a>(&self, article: &'a str) -> &'a str {
        article
            .strip_prefix(self.root_dir())
            .and_then(|part| part.strip_prefix('/'))
            .unwrap_or(article)
    }
    pub(super) fn cloudflare_dashboard(&self) -> String {
        match &self.site {
            Some(site) if !site.account.is_empty() => format!(
                "https://dash.cloudflare.com/{}/pages/view/{}",
                site.account, site.pages_project
            ),
            Some(site) => format!(
                "https://dash.cloudflare.com/?to=/:account/pages/view/{}",
                site.pages_project
            ),
            None => "https://dash.cloudflare.com/".into(),
        }
    }
    pub fn startup(&mut self) -> Reply<Startup> {
        let wizard = self.read_wizard()?;

        if self.site.is_none() && wizard.is_none() && self.root.join("source/.git/config").is_file()
        {
            let origin = self.git(
                Some(&self.source()),
                &["config", "--local", "--get", "remote.origin.url"],
            );
            if origin
                .as_deref()
                .ok()
                .and_then(|value| github_remote(value).ok())
                .as_deref()
                == Some(BLOG)
            {
                self.activate_site(SiteConfig::legacy())?;
            }
        }
        Ok(Startup {
            site: self.site.clone(),
            wizard,
            show_welcome: !self
                .storage
                .get::<bool>(SETTINGS, WELCOME_ACKNOWLEDGED)?
                .unwrap_or(false),
            pages_build: PAGES_BUILD,
        })
    }

    fn activate_site(&mut self, site: SiteConfig) -> Reply<()> {
        self.storage.put(SETTINGS, "site", &site)?;

        self.blog_remote.clone_from(&site.remote);
        self.site = Some(site);
        self.stop_preview();
        Ok(())
    }
    fn read_wizard(&self) -> Reply<Option<Wizard>> {
        let wizard: Option<Wizard> = self.storage.get(SETTINGS, "onboarding")?;
        if wizard.as_ref().is_some_and(|wizard| !uuid(&wizard.id)) {
            return Err("首次设置进度无效。".into());
        }
        Ok(wizard.filter(|wizard| wizard.existing_site == self.site.is_some()))
    }
    fn save_wizard(&self, wizard: &Wizard) -> Reply<()> {
        self.storage.put(SETTINGS, "onboarding", wizard)
    }
    fn setup_checkout(&self, wizard: &Wizard) -> PathBuf {
        self.root
            .join("onboarding")
            .join(&wizard.id)
            .join("checkout")
    }

    pub fn wizard_action(
        &mut self,
        action: Action,
        input: Option<Input>,
        github_token: Option<String>,
        pages_token: Option<String>,
    ) -> Reply<Startup> {
        self.stop_preview();
        if matches!(action, Action::Continue) {
            if self.site.is_none() {
                return Err("请先完成站点设置。".into());
            }
            self.storage.put(SETTINGS, WELCOME_ACKNOWLEDGED, &true)?;
            return self.startup();
        }
        if let Action::Start(route) = action {
            let active = self.site.as_ref();
            if active.is_some() && route == Route::Create {
                return Err("此客户端已关联一个站点。".into());
            }
            let wizard = Wizard {
                id: Uuid::new_v4().to_string(),
                route,
                existing_site: active.is_some(),
                step: 1,
                input: Input {
                    hugo_path: self.preferences.hugo_path.clone(),
                    proxy: self.preferences.proxy.clone(),
                    ..active.map_or_else(Input::default, |site| Input {
                        title: site.title.clone(),
                        remote: site.remote.clone(),
                        branch: site.branch.clone(),
                        root_dir: site.root_dir.clone(),
                        posts_dir: site.posts_dir.clone(),
                        website: site.website.clone(),
                        pages_project: site.pages_project.clone(),
                        pages_url: site.pages_url.clone(),
                        account: site.account.clone(),
                        ..Input::default()
                    })
                },
                checkout_remote: active.map_or_else(String::new, |site| site.remote.clone()),
                checkout_branch: active.map_or_else(String::new, |site| site.branch.clone()),
                first_commit: String::new(),
                hugo_version: String::new(),
                theme: String::new(),
                candidates: Vec::new(),
                articles: 0,
                unsupported: 0,
                sample: None,
                inspected: None,
                checked: None,
                deployment: String::new(),
            };
            self.save_wizard(&wizard)?;
            return self.startup();
        }
        let mut wizard = self.read_wizard()?.ok_or("请先选择设置方式。")?;
        if let Some(mut input) = input {
            // Keep the recorded identity of a generated site after a push attempt.
            if wizard.route == Route::Create && !wizard.first_commit.is_empty() {
                input.title.clone_from(&wizard.input.title);
                input.hugo_path.clone_from(&wizard.input.hugo_path);
            }
            wizard.input = input;
        }
        self.save_wizard(&wizard)?;
        match action {
            Action::Save if wizard.step < 4 => {}
            Action::Back if wizard.step > 1 => {
                wizard.step -= 1;
                wizard.checked = None;
            }
            Action::Generate if wizard.route == Route::Create && wizard.step == 1 => {
                self.generate(&mut wizard)?
            }
            Action::Connect if wizard.route == Route::Import && wizard.step == 1 => {
                self.connect(&mut wizard, github_token.as_deref())?
            }
            Action::Detect if wizard.route == Route::Import && wizard.step == 2 => {
                self.inspect_setup(&mut wizard, false)?
            }
            Action::Inspect if wizard.route == Route::Import && wizard.step == 2 => {
                self.inspect_setup(&mut wizard, true)?
            }
            Action::Push if wizard.route == Route::Create && wizard.step == 2 => {
                self.first_push(&mut wizard, github_token.as_deref())?
            }
            Action::Verify if wizard.step == 3 => {
                self.verify_setup(&mut wizard, pages_token.as_deref())?
            }
            Action::Finish if wizard.step == 4 => {
                let site = wizard.checked.ok_or("请先完成站点验证。")?;
                let checkout = self
                    .root
                    .join("onboarding")
                    .join(&wizard.id)
                    .join("checkout");

                if let Some(active) = &self.site {
                    if site.remote != active.remote
                        || site.branch != active.branch
                        || site.root_dir != active.root_dir
                        || site.posts_dir != active.posts_dir
                        || site.content_dir != active.content_dir
                    {
                        return Err(
                            "现有站点的仓库、分支和文章目录必须保持一致，以便继续使用本地草稿。"
                                .into(),
                        );
                    }
                    self.activate_site(site)?;
                    self.storage.remove(SETTINGS, "onboarding")?;
                } else {
                    let source = self.source();
                    if checkout.is_dir() {
                        if source.exists() {
                            return Err(
                                "本地已有站点目录，无法覆盖。请检查旧版站点是否已迁移。".into()
                            );
                        }
                        fs::rename(&checkout, &source).map_err(error)?;
                    } else if !source.join(".git").is_dir() {
                        return Err("找不到已验证的本地工作副本。".into());
                    }
                    self.activate_site(site)?;
                }
                self.storage.put(SETTINGS, WELCOME_ACKNOWLEDGED, &true)?;
                return self.startup();
            }
            _ => return Err("当前步骤不能执行此操作。".into()),
        }
        self.save_wizard(&wizard)?;
        self.startup()
    }

    fn use_setup_tools(&mut self, input: &Input) -> Reply<HugoInfo> {
        let mut settings = self.preferences.clone();
        settings.proxy.clone_from(&input.proxy);
        settings.hugo_path.clone_from(&input.hugo_path);
        self.settings(Some(settings))?;
        self.inspect_hugo(&self.preferences.hugo_path)
    }
    fn check_git(&self) -> Reply<()> {
        self.git(None, &["--version"])
            .map_err(|_| "找不到 Git。请安装 Git for Windows，再点击按钮重试。".to_owned())?;
        Ok(())
    }
    fn fresh_checkout(&self, wizard: &Wizard) -> Reply<TempDir> {
        let work = self.setup_checkout(wizard);
        let parent = work.parent().ok_or("设置工作区路径无效。")?;
        fs::create_dir_all(parent).map_err(error)?;
        tempfile::tempdir_in(parent).map_err(error)
    }
    fn install_checkout(&self, wizard: &Wizard, staged: &Path, temp: &TempDir) -> Reply<()> {
        let checkout = self.setup_checkout(wizard);
        let previous = temp.path().join("previous");
        if checkout.is_dir() {
            fs::rename(&checkout, &previous).map_err(error)?;
        }
        if let Err(reason) = fs::rename(staged, &checkout) {
            if previous.is_dir() {
                fs::rename(previous, checkout).map_err(error)?;
            }
            return Err(error(reason));
        }
        Ok(())
    }
    fn generate(&mut self, wizard: &mut Wizard) -> Reply<()> {
        if !wizard.first_commit.is_empty() {
            wizard.step = 2;
            return Ok(());
        }
        let info = self.use_setup_tools(&wizard.input)?;
        if !info.extended {
            return Err("请选择 Hugo Extended，或使用软件内置的版本。".into());
        }
        self.check_git()?;
        let title = wizard.input.title.trim();
        if title.is_empty() || title.chars().count() > 100 || title.contains(['\r', '\n']) {
            return Err("请输入 1–100 字的博客名称。".into());
        }
        let temp = self.fresh_checkout(wizard)?;
        let staged = temp.path().join("staged");
        fs::create_dir(&staged).map_err(error)?;
        starter(&staged, title)?;
        self.git(None, &["init", "-b", "main", path_text(&staged)?])?;
        self.git(Some(&staged), &["add", "--", "."])?;
        self.commit(&staged, "Create Hugo blog")?;
        self.setup_build(&staged, "")?;
        self.install_checkout(wizard, &staged, &temp)?;
        wizard.input.branch = "main".into();
        wizard.input.posts_dir = "content/post".into();
        wizard.checkout_branch = "main".into();
        wizard.hugo_version = version_number(&info.version)?;
        wizard.sample = Some("content/post/hello/index.md".into());
        wizard.step = 2;
        Ok(())
    }

    fn connect(&mut self, wizard: &mut Wizard, token: Option<&str>) -> Reply<()> {
        self.check_git()?;
        let mut settings = self.preferences.clone();
        settings.proxy.clone_from(&wizard.input.proxy);
        self.settings(Some(settings))?;
        let remote = github_remote(&wizard.input.remote)?;
        if self.site.as_ref().is_some_and(|active| {
            remote != active.remote || wizard.input.branch.trim() != active.branch
        }) {
            return Err("此向导用于重新连接当前站点，请保留现有仓库和发布分支。".into());
        }
        let temp = self.fresh_checkout(wizard)?;
        let staged = temp.path().join("staged");

        let branch = {
            let requested = wizard.input.branch.trim();
            let branch = if requested.is_empty() {
                let refs = self.github_git(
                    None,
                    &remote,
                    token,
                    &["ls-remote", "--symref", &remote, "HEAD"],
                )?;
                refs.lines()
                    .find_map(|line| {
                        line.strip_prefix("ref: refs/heads/")?
                            .strip_suffix("\tHEAD")
                    })
                    .ok_or("该仓库没有默认分支，请检查仓库是否为空。")?
                    .to_owned()
            } else {
                requested.to_owned()
            };
            if branch.starts_with('-')
                || self
                    .git(None, &["check-ref-format", &format!("refs/heads/{branch}")])
                    .is_err()
            {
                return Err("GitHub 分支名称无效。".into());
            }
            self.github_git(
                None,
                &remote,
                token,
                &[
                    "clone",
                    "--single-branch",
                    "--recurse-submodules",
                    "--branch",
                    &branch,
                    "--",
                    &remote,
                    path_text(&staged)?,
                ],
            )?;
            if let Some(token) = nonempty_token(token) {
                save_secret(&github_key(&remote), token)?;
            }
            branch
        };
        if wizard.checkout_remote != remote || wizard.checkout_branch != branch {
            wizard.input.title.clear();
            wizard.input.root_dir.clear();
            wizard.input.posts_dir.clear();
            wizard.input.website.clear();
            wizard.input.pages_url.clear();
            wizard.input.pages_project.clear();
            wizard.input.account.clear();
            wizard.input.confirmed = false;
        }
        self.install_checkout(wizard, &staged, &temp)?;
        wizard.checkout_remote = remote.clone();
        wizard.checkout_branch = branch.clone();
        wizard.input.remote = remote;
        wizard.input.branch = branch;
        wizard.inspected = None;
        wizard.sample = None;
        wizard.candidates.clear();
        wizard.step = 2;
        let _ = self.inspect_setup(wizard, false);
        Ok(())
    }

    fn inspect_setup(&mut self, wizard: &mut Wizard, build: bool) -> Reply<()> {
        let info = self.use_setup_tools(&wizard.input)?;
        let checkout = self.setup_checkout(wizard);
        let root = normalize_relative(&wizard.input.root_dir);
        let source = subdir(&checkout, &root)?;
        let mut command = self.setup_hugo(&checkout, &root)?;
        command.args(["config", "--format", "json"]);
        let config: Value =
            serde_json::from_str(&run(command, "读取 Hugo 配置")?).map_err(error)?;
        let content_dir = config_dir(&config, "contentdir", "content");
        let themes_dir = config_dir(&config, "themesdir", "themes");
        let content = subdir(&source, &content_dir)?;
        subdir(&source, &themes_dir)?;
        let mut candidates = Vec::new();
        if let Ok(entries) = fs::read_dir(&content) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|item| item.is_dir()) {
                    if let Some(name) = entry
                        .file_name()
                        .to_str()
                        .filter(|name| !name.starts_with('.'))
                    {
                        candidates.push(repo_path(&root, &format!("{content_dir}/{name}")));
                    }
                }
            }
        }
        candidates.sort();
        let selected = normalize_relative(&wizard.input.posts_dir);
        let posts = if selected.is_empty() {
            ["post", "posts"]
                .into_iter()
                .map(|name| repo_path(&root, &format!("{content_dir}/{name}")))
                .find(|name| candidates.contains(name))
                .or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()))
                .unwrap_or_default()
        } else {
            selected
        };
        let content_prefix = repo_path(&root, &content_dir);
        if !posts.is_empty()
            && posts != content_prefix
            && !posts.starts_with(&format!("{content_prefix}/"))
        {
            return Err(format!("请选择 {content_prefix}/ 下的文章目录。"));
        }
        let (articles, unsupported, sample) = if posts.is_empty() {
            (0, 0, None)
        } else {
            inspect_articles(&checkout, &posts)?
        };
        if build {
            if posts.is_empty() {
                return Err("请选择文章目录。".into());
            }
            self.setup_build(&checkout, &root)?;
        }
        let theme = match &config["theme"] {
            Value::String(value) => value.clone(),
            Value::Array(values) => values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("、"),
            _ => String::new(),
        };
        let website = provided(
            &wizard.input.website,
            config["baseurl"]
                .as_str()
                .filter(|url| *url != "/")
                .unwrap_or_default(),
        );
        let title = provided(
            &wizard.input.title,
            config["title"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("我的博客"),
        );
        wizard.input.title.clone_from(&title);
        wizard.input.website.clone_from(&website);
        wizard.input.root_dir.clone_from(&root);
        wizard.input.posts_dir.clone_from(&posts);
        wizard.hugo_version = version_number(&info.version)?;
        wizard.theme = theme;
        wizard.candidates = candidates;
        wizard.articles = articles;
        wizard.unsupported = unsupported;
        wizard.sample = sample;
        if build {
            wizard.inspected = Some(SiteConfig {
                title,
                remote: wizard.checkout_remote.clone(),
                branch: wizard.checkout_branch.clone(),
                root_dir: root,
                content_dir,
                posts_dir: posts,
                themes_dir,
                website,
                pages_project: String::new(),
                pages_url: String::new(),
                account: String::new(),
                hosted_images: wizard.checkout_remote == BLOG,
            });
            wizard.step = 3;
        }
        Ok(())
    }
    fn setup_hugo(&self, checkout: &Path, root: &str) -> Reply<Command> {
        let root = subdir(checkout, root)?;
        if !root.is_dir() {
            return Err("找不到 Hugo 站点根目录。".into());
        }
        let mut cmd = process(self.hugo_path(&self.preferences.hugo_path)?);
        cmd.current_dir(&root).arg("--source").arg(&root);
        cmd.env("HUGO_CACHEDIR", self.root.join("hugo-cache"));
        if !self.preferences.proxy.is_empty() {
            cmd.env("HTTP_PROXY", &self.preferences.proxy)
                .env("HTTPS_PROXY", &self.preferences.proxy)
                .env("NO_PROXY", "localhost,127.0.0.1");
        }
        Ok(cmd)
    }
    fn setup_build(&self, checkout: &Path, root: &str) -> Reply<()> {
        let output = tempfile::tempdir_in(&self.root).map_err(error)?;
        let mut command = self.setup_hugo(checkout, root)?;
        command.arg("--destination").arg(output.path());
        run(command, "本地 Hugo 构建")?;
        if !output.path().join("index.html").is_file() {
            return Err("Hugo 构建没有生成首页。请检查站点根目录、主题和站点配置。".into());
        }
        Ok(())
    }

    fn first_push(&self, wizard: &mut Wizard, token: Option<&str>) -> Reply<()> {
        let remote = github_remote(&wizard.input.remote)?;
        let checkout = self.setup_checkout(wizard);
        let commit = self.revision(&checkout)?;
        if !wizard.first_commit.is_empty() && wizard.checkout_remote != remote {
            return Err(
                "已记录一次首次推送。请使用原仓库地址重试，或回到欢迎页重新创建站点。".into(),
            );
        }
        let expected = format!("{commit}\trefs/heads/main");

        let uploaded = {
            let refs = self.github_git(None, &remote, token, &["ls-remote", "--refs", &remote])?;
            let uploaded = refs.lines().any(|line| line == expected);
            if !uploaded && !refs.trim().is_empty() {
                return Err("仓库已有内容。请选择空仓库，或改用「导入已有站点」。".into());
            }
            if let Some(token) = nonempty_token(token) {
                save_secret(&github_key(&remote), token)?;
            }
            uploaded
        };
        self.git(Some(&checkout), &["config", "remote.origin.url", &remote])?;
        self.git(
            Some(&checkout),
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        )?;
        wizard.checkout_remote = remote.clone();
        wizard.first_commit = commit;
        wizard.input.remote = remote.clone();
        wizard.input.branch = "main".into();
        self.save_wizard(wizard)?;

        {
            if !uploaded {
                // Require an absent branch so a concurrent push cannot be overwritten.
                self.github_git(
                    Some(&checkout),
                    &remote,
                    token,
                    &[
                        "push",
                        "--force-with-lease=refs/heads/main:",
                        &remote,
                        "HEAD:refs/heads/main",
                    ],
                )?;
            }
            let head = self.github_git(
                None,
                &remote,
                token,
                &["ls-remote", "--refs", &remote, "refs/heads/main"],
            )?;
            if !head.lines().any(|line| line == expected) {
                return Err("尚未在 GitHub 确认首次提交。请重试。".into());
            }
        }
        wizard.step = 3;
        Ok(())
    }

    fn verify_setup(&self, wizard: &mut Wizard, token: Option<&str>) -> Reply<()> {
        let input = &wizard.input;
        let pages = pages_url(&input.pages_url)?;
        let host_project = pages
            .host_str()
            .unwrap_or_default()
            .trim_end_matches(".pages.dev");
        let project = provided(&input.pages_project, host_project);
        if !project_name(&project) {
            return Err("Cloudflare Pages 项目名无效。".into());
        }
        let configured_site = if wizard.route == Route::Create {
            pages.clone()
        } else {
            public_url(&provided(&input.website, pages.as_str()))?
        };
        let mut site = if wizard.route == Route::Create {
            SiteConfig {
                title: input.title.trim().into(),
                remote: wizard.checkout_remote.clone(),
                branch: wizard.checkout_branch.clone(),
                hosted_images: false,
                ..SiteConfig::legacy()
            }
        } else {
            wizard.inspected.clone().ok_or("请先完成 Hugo 本地构建。")?
        };
        site.website = configured_site.to_string();
        site.pages_url = pages.to_string();
        site.pages_project = project;
        site.account = input.account.trim().to_ascii_lowercase();
        if !site.account.is_empty()
            && (site.account.len() != 32 || !site.account.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("请输入 32 位 Cloudflare 账号 ID。".into());
        }
        let http = self.http()?;
        let stored = if !site.account.is_empty() && nonempty_token(token).is_none() {
            read_secret(&pages_key(&site.account))?
        } else {
            None
        };
        let auth = nonempty_token(token).or(stored.as_deref());
        let mut status = if wizard.route == Route::Create {
            let mut marker = pages.join(MARKER).map_err(error)?;
            marker
                .query_pairs_mut()
                .append_pair("commit", &wizard.first_commit);
            let response = request_ok(&http, marker, "读取首次部署标记")?;
            let body = bounded_json(
                response,
                "首次部署标记尚未生成，请检查构建命令和 Cloudflare 构建日志。",
            )?;
            if body["commit"].as_str() != Some(wizard.first_commit.as_str())
                || body["branch"].as_str() != Some("main")
            {
                return Err(
                    "Pages 生产地址尚未部署刚推送的 main 提交，请检查分支和部署进度。".into(),
                );
            }
            "首次部署已验证".to_owned()
        } else {
            if !input.confirmed && auth.is_none() {
                return Err("请在 Cloudflare 核对仓库与生产分支，或填写只读令牌。".into());
            }
            "生产地址可访问".to_owned()
        };
        request_ok(&http, configured_site, "检查生产站点首页")?;
        if wizard.route == Route::Import && site.website != site.pages_url {
            check_pages_alias(&http, pages, &site.website)?;
        }
        if let Some(token) = auth {
            status = self.pages_status(&site, token, None)?;
            if stored.is_none() {
                save_secret(&pages_key(&site.account), token)?;
            }
        }
        wizard.checked = Some(site);
        wizard.deployment = status;
        wizard.step = 4;
        Ok(())
    }
    fn pages_status(&self, site: &SiteConfig, token: &str, commit: Option<&str>) -> Reply<String> {
        validate_secret(token)?;
        if site.account.len() != 32 || !site.account.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("请输入 32 位 Cloudflare 账号 ID。".into());
        }
        let endpoint = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}",
            site.account, site.pages_project
        );
        let response = self
            .http()?
            .get(endpoint)
            .bearer_auth(token)
            .send()
            .map_err(|_| "无法连接 Cloudflare API。请检查网络或代理。")?;
        if !response.status().is_success() {
            return Err(format!(
                "Cloudflare API 返回 {}。请检查项目、账号和 Pages Read 权限。",
                response.status()
            ));
        }
        let body = bounded_json(response, "Cloudflare 返回了无法识别的项目数据。")?;
        let result = &body["result"];
        let source = &result["source"];
        let expected = github_remote(&format!(
            "{}/{}",
            source["config"]["owner"].as_str().unwrap_or_default(),
            source["config"]["repo_name"].as_str().unwrap_or_default()
        ))
        .ok();
        let branch = source["config"]["production_branch"]
            .as_str()
            .or(result["production_branch"].as_str());
        if source["type"].as_str() != Some("github")
            || expected.as_deref() != Some(site.remote.as_str())
            || branch != Some(site.branch.as_str())
        {
            return Err("Cloudflare 项目连接的 GitHub 仓库或生产分支不匹配。".into());
        }
        let expected_host = Url::parse(&site.pages_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));
        if result["subdomain"].as_str() != expected_host.as_deref() {
            return Err("填写的 Pages 生产地址与 Cloudflare 项目不匹配。".into());
        }
        let latest = &result["latest_deployment"];
        let canonical = &result["canonical_deployment"];
        let deployment = if let Some(commit) = commit {
            match [latest, canonical].into_iter().find(|item| {
                item["deployment_trigger"]["metadata"]["commit_hash"].as_str() == Some(commit)
                    && item["environment"].as_str() == Some("production")
            }) {
                Some(item) => item,
                None => return Ok("Cloudflare 等待此提交的生产部署".into()),
            }
        } else {
            canonical
        };
        Ok(match deployment["latest_stage"]["status"].as_str() {
            Some("success") => "Cloudflare 生产部署成功",
            Some("failure") => "Cloudflare 构建失败",
            _ => "Cloudflare 正在部署，可在控制台查看进度",
        }
        .into())
    }
    pub fn deployment_status(&self, commit: &str) -> Reply<Option<String>> {
        let Some(site) = self.site.as_ref().filter(|site| !site.account.is_empty()) else {
            return Ok(None);
        };
        let Some(token) = read_secret(&pages_key(&site.account))? else {
            return Ok(None);
        };
        self.pages_status(site, &token, Some(commit)).map(Some)
    }
    fn http(&self) -> Reply<Client> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none());
        if !self.preferences.proxy.is_empty() {
            builder = builder.proxy(reqwest::Proxy::all(&self.preferences.proxy).map_err(error)?);
        }
        builder.build().map_err(error)
    }
    pub fn setup_browser(&self, page: &str) -> Reply<()> {
        let target = match page {
            "git" => "https://git-scm.com/install/windows",
            "github_new" => "https://github.com/new",
            "github_token" => "https://github.com/settings/personal-access-tokens/new",
            "pages" => "https://dash.cloudflare.com/?to=/:account/workers-and-pages",
            "pages_token" => "https://dash.cloudflare.com/profile/api-tokens",
            "website" => {
                let wizard = self.read_wizard()?.ok_or("找不到引导进度。")?;
                let site = wizard.checked.ok_or("请先验证站点。")?;
                return open_in_browser(&public_url(&site.website)?);
            }
            _ => return Err("未知设置链接。".into()),
        };
        open_in_browser(&Url::parse(target).map_err(error)?)
    }
    pub fn setup_preview(&mut self) -> Reply<SetupPreview> {
        let wizard = self.read_wizard()?.ok_or("找不到引导进度。")?;
        let checkout = self.setup_checkout(&wizard);
        self.stop_preview();
        let dir = tempfile::tempdir_in(&self.root).map_err(error)?;
        let listener = bind_preview_port(self.preferences.preview_port.unwrap_or_default())?;
        let address = listener.local_addr().map_err(error)?;
        let home = format!("http://{address}/");
        let log = dir.path().join("hugo.log");
        let output = fs::File::create(&log).map_err(error)?;
        let stderr = output.try_clone().map_err(error)?;
        let mut cmd = self.setup_hugo(&checkout, &wizard.input.root_dir)?;
        cmd.args([
            "server",
            "-D",
            "--bind",
            "127.0.0.1",
            "--port",
            &address.port().to_string(),
            "--baseURL",
            &home,
            "--appendPort=false",
            "--disableLiveReload",
            "--noHTTPCache",
            "--watch=false",
        ]);
        let article = self.setup_sample(&wizard, &checkout, &home);
        drop(listener);
        let child = cmd.stdout(output).stderr(stderr).spawn().map_err(error)?;
        let mut preview = Preview {
            child,
            dir,
            log,
            origin: home.clone(),
            id: String::new(),
            path: String::new(),
            revision: String::new(),
            images: HashSet::new(),
        };
        wait_preview(&mut preview, address)?;
        self.preview = Some(preview);
        Ok(SetupPreview { home, article })
    }
    fn setup_sample(&self, wizard: &Wizard, checkout: &Path, origin: &str) -> Option<String> {
        let path = wizard.sample.as_deref()?;
        let relative = path
            .strip_prefix(&wizard.input.root_dir)
            .and_then(|path| path.strip_prefix('/'))
            .unwrap_or(path);
        let mut cmd = self.setup_hugo(checkout, &wizard.input.root_dir).ok()?;
        cmd.args(["list", "all"]).env("HUGO_BASEURL", origin);
        let list = run(cmd, "读取 Hugo 文章地址").ok()?;
        let mut csv = csv::ReaderBuilder::new().from_reader(list.as_bytes());
        let header = csv.headers().ok()?;
        let path_column = header.iter().position(|name| name == "path")?;
        let url_column = header.iter().position(|name| name == "permalink")?;
        let base = Url::parse(origin).ok()?;
        for record in csv.records().flatten() {
            let entry = record.get(path_column)?.replace('\\', "/");
            if entry == relative
                || relative
                    .strip_suffix(&entry)
                    .is_some_and(|prefix| prefix.ends_with('/'))
            {
                let url = Url::parse(record.get(url_column)?).ok()?;
                if url.origin() == base.origin() {
                    return Some(url.into());
                }
            }
        }
        None
    }

    pub(super) fn site_git(&self, repo: Option<&Path>, args: &[&str]) -> Reply<String> {
        if self.blog_remote.starts_with("https://github.com/") {
            self.github_git(repo, &self.blog_remote, None, args)
        } else {
            self.git(repo, args)
        }
    }

    fn github_git(
        &self,
        repo: Option<&Path>,
        remote: &str,
        explicit: Option<&str>,
        args: &[&str],
    ) -> Reply<String> {
        let cached = if nonempty_token(explicit).is_none() {
            read_secret(&github_key(remote))?
        } else {
            None
        };
        let proxy = self.github_proxy(repo, remote);
        let mut cmd = self.git_command(repo);
        proxy.apply(&mut cmd, remote);
        cmd.args([
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "credential.useHttpPath=true",
        ]);
        if let Some(token) = nonempty_token(explicit).or(cached.as_deref()) {
            validate_secret(token)?;
            let basic =
                base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
            cmd.env_remove("GIT_CONFIG_PARAMETERS")
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", format!("http.{remote}.extraHeader"))
                .env(
                    "GIT_CONFIG_VALUE_0",
                    format!("Authorization: Basic {basic}"),
                );
            cmd.args(["-c", "http.followRedirects=false"]);
            let outcome = cmd.args(args).output().map_err(|_| "无法启动 Git。")?;
            if outcome.status.success() {
                return Ok(String::from_utf8_lossy(&outcome.stdout).into_owned());
            }
            return Err(format!(
                "Git {} 失败。请检查网络、仓库地址、分支和令牌权限。",
                args[0]
            ));
        }
        cmd.args(args);
        run(cmd, "GitHub 操作").map_err(|message| proxy.explain_auth_error(message))
    }
}

fn wait_preview(preview: &mut Preview, address: std::net::SocketAddr) -> Reply<()> {
    let deadline = Instant::now() + Duration::from_secs(16);
    loop {
        if preview.child.try_wait().map_err(error)?.is_some() || Instant::now() > deadline {
            let logs = fs::read_to_string(&preview.log).unwrap_or_default();
            return Err(format!("Hugo 预览服务未启动。\n{logs}"));
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(80)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn bounded_json(response: reqwest::blocking::Response, message: &str) -> Reply<Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| message.to_owned())?;
    if bytes.len() > 512 * 1024 {
        return Err(message.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| message.into())
}
fn check_pages_alias(client: &Client, pages: Url, website: &str) -> Reply<()> {
    let response = client
        .get(pages.clone())
        .send()
        .map_err(|_| "无法检查 Pages 生产地址。请检查网络或代理。")?;
    if response.status().is_success() {
        return Ok(());
    }
    // Some sites redirect their pages.dev hostname to the already verified custom homepage.
    if response.status().is_redirection() {
        let target = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| pages.join(value).ok());
        if target.is_some_and(|url| url.as_str() == website) {
            return Ok(());
        }
    }
    Err(format!(
        "检查 Pages 生产地址返回 {}。请核对控制台中的生产地址。",
        response.status()
    ))
}
fn request_ok(http: &Client, url: Url, label: &str) -> Reply<reqwest::blocking::Response> {
    let value = http
        .get(url)
        .send()
        .map_err(|_| format!("{label}失败。请检查地址、网络或代理。"))?;
    if !value.status().is_success() {
        return Err(format!(
            "{label}返回 {}。请检查生产地址和部署状态。",
            value.status()
        ));
    }
    Ok(value)
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !matches!(value, "." | "..")
}
fn project_name(value: &str) -> bool {
    identifier(value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn github_remote(value: &str) -> Reply<String> {
    let value = value.trim().trim_end_matches('/');
    let short = value.strip_prefix("https://github.com/").unwrap_or(value);
    let mut parts = short.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default().trim_end_matches(".git");
    if !identifier(owner) || !identifier(repo) || parts.next().is_some() {
        return Err(
            "请输入 GitHub 仓库地址，例如 owner/blog 或 https://github.com/owner/blog。".into(),
        );
    }
    Ok(format!(
        "https://github.com/{}/{}.git",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    ))
}
fn public_url(value: &str) -> Reply<Url> {
    let mut url = Url::parse(value.trim()).map_err(|_| "请输入完整的 HTTPS 站点地址。")?;
    if url.scheme() != "https"
        || !matches!(url.host(), Some(url::Host::Domain(domain)) if domain.contains('.'))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some()
    {
        return Err("请输入不带凭据、端口、查询或片段的 HTTPS 地址。".into());
    }
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.pop_if_empty().push("");
    }
    Ok(url)
}
fn pages_url(value: &str) -> Reply<Url> {
    let url = public_url(value)?;
    let project = url
        .host_str()
        .and_then(|host| host.strip_suffix(".pages.dev"))
        .unwrap_or_default();
    if !project_name(project) || url.path() != "/" {
        return Err("请输入 Pages 生产站点首页，例如 https://my-blog.pages.dev/。".into());
    }
    Ok(url)
}
fn normalize_relative(value: &str) -> String {
    let value = value.trim().replace('\\', "/");
    let value = value.trim_end_matches('/');
    if value == "." {
        String::new()
    } else {
        value.to_owned()
    }
}
fn subdir(root: &Path, relative: &str) -> Reply<PathBuf> {
    if relative.is_empty() {
        Ok(root.to_path_buf())
    } else {
        safe_join(root, relative)
    }
}
fn repo_path(root: &str, relative: &str) -> String {
    if root.is_empty() {
        relative.into()
    } else {
        format!("{root}/{relative}")
    }
}
fn config_dir(config: &Value, name: &str, default: &str) -> String {
    normalize_relative(
        config[name]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(default),
    )
}
fn provided(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_owned()
    } else {
        value.trim().to_owned()
    }
}
fn version_number(output: &str) -> Reply<String> {
    let version = output
        .split_whitespace()
        .nth(1)
        .and_then(|part| part.strip_prefix('v'))
        .unwrap_or_default();
    let version = version.split(['+', '-']).next().unwrap_or_default();
    if version.split('.').count() != 3
        || version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("无法识别 Hugo 版本号。".into());
    }
    Ok(version.into())
}
fn inspect_articles(checkout: &Path, posts: &str) -> Reply<(usize, usize, Option<String>)> {
    let root = safe_join(checkout, posts)?;
    let mut total = 0;
    let mut unsupported = 0;
    let mut sample = None;
    if root.is_dir() {
        for item in WalkDir::new(&root).follow_links(false).sort_by_file_name() {
            let item = item.map_err(error)?;
            let path = item.path();
            if !item.file_type().is_file()
                || path.extension().is_none_or(|ext| ext != "md")
                || is_section_index(path)
            {
                continue;
            }
            total += 1;
            let source = fs::read_to_string(path).map_err(error)?;
            let yaml = source
                .trim_start_matches('\u{feff}')
                .lines()
                .next()
                .is_some_and(|line| line.trim_end() == "---");
            if !yaml {
                unsupported += 1;
            }
            if sample.is_none() {
                sample = Some(
                    path_text(path.strip_prefix(checkout).map_err(error)?)?.replace('\\', "/"),
                );
            }
        }
    }
    Ok((total, unsupported, sample))
}
fn starter(root: &Path, title: &str) -> Reply<()> {
    let title = serde_json::to_string(title).map_err(error)?;
    let config = format!(
        "baseURL: /\ntitle: {title}\nlanguageCode: zh-CN\npermalinks:\n  post: /p/:slug/\n"
    );
    for (path, text) in [
        ("hugo.yaml", config.as_str()),
        (".gitignore", "/public/\n/resources/_gen/\n/.hugo_build.lock\n"),
        (".gitattributes", "* text=auto eol=crlf\n"),
        ("content/post/hello/index.md", "---\ntitle: 你好，世界\nslug: hello\ndraft: false\n---\n\n这是你的第一篇文章。打开博客 CMS 开始写作。\n"),
        ("layouts/_default/baseof.html", include_str!("starter/baseof.html")),
        ("layouts/index.html", include_str!("starter/home.html")),
        ("layouts/_default/list.html", include_str!("starter/list.html")),
        ("layouts/_default/single.html", include_str!("starter/single.html")),
        ("static/blog.css", include_str!("starter/blog.css")),
    ] { atomic_write(&safe_join(root, path)?, crlf(text).as_bytes())?; }
    Ok(())
}

fn github_key(remote: &str) -> String {
    format!("Hugo CMS/GitHub/{remote}")
}
fn pages_key(account: &str) -> String {
    format!("Hugo CMS/Cloudflare/{}", account.to_ascii_lowercase())
}
fn nonempty_token(token: Option<&str>) -> Option<&str> {
    token.map(str::trim).filter(|value| !value.is_empty())
}
fn validate_secret(token: &str) -> Reply<()> {
    if token.is_empty()
        || token.len() > 512
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err("令牌格式无效。请只粘贴令牌本身。".into());
    }
    Ok(())
}
#[cfg(windows)]
fn save_secret(key: &str, value: &str) -> Reply<()> {
    use windows_sys::Win32::Security::Credentials::*;
    validate_secret(value)?;
    let mut target: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: value.len() as u32,
        CredentialBlob: value.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..CREDENTIALW::default()
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err("无法保存令牌到 Windows 凭据管理器。".into());
    }
    Ok(())
}
#[cfg(windows)]
fn read_secret(key: &str) -> Reply<Option<String>> {
    use windows_sys::Win32::Security::Credentials::*;
    let target: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let mut data = std::ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut data) } == 0 {
        if io::Error::last_os_error().raw_os_error() == Some(1168) {
            return Ok(None);
        }
        return Err("无法读取 Windows 凭据管理器。".into());
    }
    let result = unsafe {
        let size = (*data).CredentialBlobSize as usize;
        let result = if size == 0 {
            Ok(None)
        } else {
            let bytes = std::slice::from_raw_parts((*data).CredentialBlob, size);
            std::str::from_utf8(bytes)
                .map(|value| Some(value.to_owned()))
                .map_err(|_| "Windows 凭据无效。".into())
        };
        CredFree(data.cast());
        result
    };
    result
}
#[cfg(not(windows))]
fn read_secret(_: &str) -> Reply<Option<String>> {
    Ok(None)
}
#[cfg(not(windows))]
fn save_secret(_: &str, _: &str) -> Reply<()> {
    Err("手动令牌存储需要 Windows 凭据管理器。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/hugo-x86_64-pc-windows-msvc.exe")
    }

    #[test]
    fn legacy_workspace_reopens_at_welcome_then_resumes_without_overwriting_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cms");
        let mut workspace = Workspace::new(root.clone(), binary()).unwrap();
        assert!(workspace.startup().unwrap().show_welcome);
        assert!(workspace
            .wizard_action(Action::Continue, None, None, None)
            .is_err());
        let source = workspace.source();
        fs::create_dir_all(source.join(".git")).unwrap();
        workspace.git(Some(&source), &["init"]).unwrap();
        workspace
            .git(Some(&source), &["remote", "add", "origin", BLOG])
            .unwrap();
        fs::write(source.join("my-uncommitted-file"), "keep").unwrap();
        let startup = workspace.startup().unwrap();
        assert!(startup.show_welcome);
        assert_eq!(startup.site.unwrap().remote, BLOG);

        let mut reopened = Workspace::new(root.clone(), binary()).unwrap();
        assert!(reopened.startup().unwrap().show_welcome);
        let setup = reopened
            .wizard_action(Action::Start(Route::Import), None, None, None)
            .unwrap();
        let mut wizard = setup.wizard.unwrap();
        assert_eq!(wizard.input.remote, BLOG);
        assert_eq!(wizard.input.branch, "main");
        assert_eq!(wizard.input.posts_dir, "content/post");
        let continued = reopened
            .wizard_action(Action::Continue, None, None, None)
            .unwrap();
        assert!(!continued.show_welcome);
        assert_eq!(continued.wizard.unwrap().id, wizard.id);
        wizard.step = 4;
        let mut checked = SiteConfig::legacy();
        checked.branch = "other".into();
        wizard.checked = Some(checked.clone());
        reopened.save_wizard(&wizard).unwrap();
        assert!(reopened
            .wizard_action(Action::Finish, None, None, None)
            .is_err());
        checked.branch = "main".into();
        checked.pages_project = "updated".into();
        wizard.checked = Some(checked);
        reopened.save_wizard(&wizard).unwrap();
        let finished = reopened
            .wizard_action(Action::Finish, None, None, None)
            .unwrap();
        assert_eq!(finished.site.unwrap().pages_project, "updated");
        assert!(finished.wizard.is_none());
        assert_eq!(
            fs::read_to_string(source.join("my-uncommitted-file")).unwrap(),
            "keep"
        );
        assert!(
            !Workspace::new(root, binary())
                .unwrap()
                .startup()
                .unwrap()
                .show_welcome
        );
    }

    #[test]
    fn rejects_malformed_repository_and_preview_addresses() {
        assert_eq!(
            github_remote("https://github.com/Hengxin007/blog.git/").unwrap(),
            BLOG
        );
        for value in [
            "https://github.com.evil.test/a/b",
            "owner/-repo/extra",
            "file:///some/repo",
            "https://user@github.com/a/b",
            "owner/repo?password=x",
        ] {
            assert!(github_remote(value).is_err(), "{value}");
        }
        for value in [
            "https://branch.site.pages.dev/",
            "https://site.pages.dev/not-home",
            "https://pages.dev/",
            "https://user@site.pages.dev/",
        ] {
            assert!(pages_url(value).is_err(), "{value}");
        }
        assert_eq!(
            pages_url("https://site.pages.dev").unwrap().as_str(),
            "https://site.pages.dev/"
        );
    }

    #[test]
    fn generates_and_resumes_a_real_local_site() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cms");
        let mut workspace = Workspace::new(root.clone(), binary()).unwrap();
        assert!(workspace.startup().unwrap().site.is_none());
        workspace
            .wizard_action(Action::Start(Route::Create), None, None, None)
            .unwrap();
        let state = workspace
            .wizard_action(
                Action::Generate,
                Some(Input {
                    title: "第一次写作".into(),
                    ..Input::default()
                }),
                None,
                None,
            )
            .unwrap();
        let wizard = state.wizard.unwrap();
        assert_eq!(wizard.step, 2);
        let rev = workspace
            .revision(&workspace.setup_checkout(&wizard))
            .unwrap();
        let mut reopened = Workspace::new(root, binary()).unwrap();
        let wizard = reopened.startup().unwrap().wizard.unwrap();
        assert_eq!(wizard.input.title, "第一次写作");
        assert_eq!(
            rev,
            reopened
                .revision(&reopened.setup_checkout(&wizard))
                .unwrap()
        );
        let checkout = reopened.setup_checkout(&wizard);
        let draft = "content/post/hello/index.md";
        let mut git = reopened.git_command(None);
        git.args([
            "init",
            "--bare",
            path_text(&temp.path().join("origin.git")).unwrap(),
        ]);
        run(git, "init isolated test remote").unwrap();
        let isolated = temp.path().join("origin.git");
        reopened
            .git(
                Some(&checkout),
                &[
                    "push",
                    "--force-with-lease=refs/heads/main:",
                    path_text(&isolated).unwrap(),
                    "HEAD:refs/heads/main",
                ],
            )
            .unwrap();
        assert_eq!(
            reopened
                .git(Some(&isolated), &["rev-parse", "refs/heads/main"])
                .unwrap()
                .trim(),
            rev
        );
        let preview = reopened.setup_preview().unwrap();
        assert!(preview.home.starts_with("http://127.0.0.1:"));
        let article = preview.article.unwrap();
        assert!(article.ends_with("/p/hello/"), "{article}");
        assert_eq!(
            fs::read(checkout.join("hugo.yaml")).unwrap()[..8],
            *b"baseURL:"
        );
        assert!(checkout.join(draft).is_file());
    }

    #[test]
    fn imports_subfolder_on_non_main_branch_without_touching_origin() {
        let temp = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(temp.path().join("cms"), binary()).unwrap();
        workspace
            .wizard_action(Action::Start(Route::Import), None, None, None)
            .unwrap();
        let mut wizard = workspace.read_wizard().unwrap().unwrap();
        let repo = temp.path().join("existing");
        let website = repo.join("website");
        starter(&website, "已有博客").unwrap();
        fs::rename(website.join("content/post"), website.join("content/posts")).unwrap();
        fs::write(website.join("hugo.yaml"), crlf("baseURL: https://example.com/blog/\ntitle: 已有博客\npermalinks:\n  posts: /articles/:slug/\n")).unwrap();
        fs::write(
            website.join("content/posts/legacy.md"),
            crlf("+++\ntitle = '旧文章'\n+++\n\n旧文内容\n"),
        )
        .unwrap();
        workspace
            .git(
                None,
                &["init", "-b", "production", path_text(&repo).unwrap()],
            )
            .unwrap();
        workspace.git(Some(&repo), &["add", "."]).unwrap();
        let original = workspace.commit(&repo, "Existing site").unwrap();
        let checkout = workspace.setup_checkout(&wizard);
        fs::create_dir_all(checkout.parent().unwrap()).unwrap();
        workspace
            .git(
                None,
                &[
                    "clone",
                    "--branch",
                    "production",
                    path_text(&repo).unwrap(),
                    path_text(&checkout).unwrap(),
                ],
            )
            .unwrap();
        wizard.checkout_remote = "https://github.com/test/existing.git".into();
        wizard.checkout_branch = "production".into();
        wizard.step = 2;
        wizard.input.root_dir = "website".into();
        workspace.inspect_setup(&mut wizard, true).unwrap();
        assert_eq!(wizard.articles, 2);
        assert_eq!(wizard.unsupported, 1);
        assert_eq!(wizard.input.website, "https://example.com/blog/");
        let site = wizard.inspected.unwrap();
        assert_eq!(site.posts_dir, "website/content/posts");
        let article = "website/content/posts/hello/index.md";
        let remote_path = path_text(&repo).unwrap().to_owned();
        fs::rename(checkout, workspace.source()).unwrap();
        workspace.activate_site(site).unwrap();
        workspace.blog_remote = remote_path;
        assert_eq!(workspace.branch(), "production");
        let source = workspace.source();
        let url = workspace
            .permalink(&source, &source, article, workspace.website())
            .unwrap();
        assert_eq!(url.as_str(), "https://example.com/blog/articles/hello/");
        assert_eq!(workspace.revision(&repo).unwrap(), original);
        assert_eq!(workspace.library(false).unwrap().articles.len(), 2);
    }

    #[test]
    fn migrates_the_legacy_site_without_moving_user_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cms");
        let mut workspace = Workspace::new(root.clone(), binary()).unwrap();
        let local = workspace.source();
        workspace
            .git(None, &["init", "-b", "main", path_text(&local).unwrap()])
            .unwrap();
        workspace
            .git(Some(&local), &["remote", "add", "origin", BLOG])
            .unwrap();
        let draft = root.join("drafts/unmodified");
        fs::write(&draft, b"draft").unwrap();
        let site = workspace.startup().unwrap().site.unwrap();
        assert_eq!(site.website, SITE);
        assert_eq!(fs::read(&draft).unwrap(), b"draft");
        assert!(local.join(".git").exists());
    }
}
