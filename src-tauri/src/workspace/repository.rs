use super::*;
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    remote: String,
    modules: Vec<Submodule>,
}

#[derive(Serialize)]
struct Submodule {
    path: String,
    url: String,
}

#[derive(Serialize)]
pub struct RepositoryRefresh {
    repository: RepositoryInfo,
    library: Library,
}

pub(super) struct PreviewModules {
    dir: TempDir,
    revision: String,
    layout: Vec<String>,
    paths: Vec<String>,
}

impl Workspace {
    pub fn repository_info(&self) -> Reply<RepositoryInfo> {
        let source = self.source();
        let modules = if source.join(".git").is_dir() {
            let revision = self.revision(&source)?;
            let snapshot = self.preview_modules_for(&source, &revision)?;
            let checkout = snapshot.map(|snapshot| snapshot.dir.path().join("site"));
            self.submodules(checkout.as_deref().unwrap_or(&source), "")?
        } else {
            Vec::new()
        };
        Ok(RepositoryInfo {
            remote: self.blog_remote.clone(),
            modules,
        })
    }

    pub fn refresh_repository(&mut self) -> Reply<RepositoryRefresh> {
        let source = self.sync(true)?;
        let modules = self.submodule_entries(&source)?;
        let (snapshot, modules) = if modules.is_empty() {
            (None, Vec::new())
        } else {
            // Keep remote submodule tips out of the source used to publish articles.
            let dir = tempfile::tempdir_in(&self.root).map_err(error)?;
            let checkout = dir.path().join("site");
            self.checkout(&source, &checkout)?;
            self.git(
                Some(&checkout),
                &["remote", "set-url", "origin", &self.blog_remote],
            )?;
            self.site_git(
                Some(&checkout),
                &[
                    "submodule",
                    "update",
                    "--init",
                    "--remote",
                    "--checkout",
                    "--recursive",
                ],
            )
            .map_err(|reason| format!("刷新子模块失败。\n{reason}"))?;
            let layout = self.submodule_layout(&checkout)?;
            let latest = self.submodules(&checkout, "")?;
            let snapshot = PreviewModules {
                dir,
                revision: self.revision(&checkout)?,
                layout,
                paths: modules.into_iter().map(|module| module.path).collect(),
            };
            (Some(snapshot), latest)
        };
        let library = self.library(false)?;
        self.stop_preview();
        self.preview_modules = snapshot;
        Ok(RepositoryRefresh {
            repository: RepositoryInfo {
                remote: self.blog_remote.clone(),
                modules,
            },
            library,
        })
    }

    fn submodule_entries(&self, repo: &Path) -> Reply<Vec<Submodule>> {
        if !repo.join(".gitmodules").is_file() {
            return Ok(Vec::new());
        }
        let config = self.git(
            Some(repo),
            &[
                "config",
                "--file",
                ".gitmodules",
                "--no-includes",
                "--null",
                "--list",
            ],
        )?;
        let mut entries = BTreeMap::<&str, (&str, &str)>::new();
        for entry in config.split_terminator('\0') {
            let Some((key, value)) = entry.split_once('\n') else {
                continue;
            };
            let Some((name, field)) = key
                .strip_prefix("submodule.")
                .and_then(|key| key.rsplit_once('.'))
            else {
                continue;
            };
            match field {
                "path" => entries.entry(name).or_default().0 = value,
                "url" => entries.entry(name).or_default().1 = value,
                _ => (),
            }
        }
        entries
            .into_iter()
            .map(|(name, (path, url))| {
                if path.is_empty() || url.is_empty() {
                    return Err(format!("子模块 {name} 缺少路径或仓库地址。"));
                }
                let resolved = self.git(
                    Some(repo),
                    &[
                        "config",
                        "--local",
                        "--get",
                        &format!("submodule.{name}.url"),
                    ],
                );
                Ok(Submodule {
                    path: path.into(),
                    url: resolved
                        .map(|url| url.trim_end().to_owned())
                        .unwrap_or_else(|_| url.into()),
                })
            })
            .collect()
    }

    fn submodules(&self, repo: &Path, prefix: &str) -> Reply<Vec<Submodule>> {
        let mut result = Vec::new();
        for mut entry in self.submodule_entries(repo)? {
            let folder = safe_join(repo, &entry.path)?;
            entry.path = format!("{prefix}{}", entry.path);
            let nested_prefix = format!("{}/", entry.path);
            result.push(entry);
            if folder.join(".git").exists() {
                result.extend(self.submodules(&folder, &nested_prefix)?);
            }
        }
        Ok(result)
    }

    fn submodule_layout(&self, repo: &Path) -> Reply<Vec<String>> {
        let tree = self.git(Some(repo), &["ls-tree", "-r", "-z", "HEAD"])?;
        Ok(tree
            .split_terminator('\0')
            .filter(|entry| entry.starts_with("160000 ") || entry.ends_with("\t.gitmodules"))
            .map(str::to_owned)
            .collect())
    }

    fn preview_modules_for(&self, source: &Path, revision: &str) -> Reply<Option<&PreviewModules>> {
        let Some(snapshot) = &self.preview_modules else {
            return Ok(None);
        };
        // Article-only commits can reuse the snapshot; changed gitlinks require another refresh.
        if snapshot.revision == revision || snapshot.layout == self.submodule_layout(source)? {
            return Ok(Some(snapshot));
        }
        Ok(None)
    }

    pub(super) fn apply_preview_modules(
        &self,
        source: &Path,
        revision: &str,
        checkout: &Path,
    ) -> Reply<bool> {
        let Some(snapshot) = self.preview_modules_for(source, revision)? else {
            return Ok(false);
        };
        let site = snapshot.dir.path().join("site");
        for path in &snapshot.paths {
            let from = safe_join(&site, path)?;
            let to = safe_join(checkout, path)?;
            for entry in WalkDir::new(&from)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| entry.file_name() != ".git")
            {
                let entry = entry.map_err(error)?;
                let target = to.join(entry.path().strip_prefix(&from).map_err(error)?);
                if entry.file_type().is_dir() {
                    fs::create_dir_all(target).map_err(error)?;
                } else if entry.file_type().is_file() {
                    fs::copy(entry.path(), target).map_err(error)?;
                } else {
                    return Err(format!(
                        "子模块预览无法复制文件：{}",
                        entry.path().display()
                    ));
                }
            }
        }
        Ok(true)
    }
}
