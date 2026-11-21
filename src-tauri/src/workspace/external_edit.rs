use super::*;
use std::io::Read;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalMarkdown {
    markdown: String,
    base_hash: String,
}

impl Workspace {
    fn external_paths(&self, id: &str) -> Reply<(PathBuf, PathBuf)> {
        if !uuid(id) {
            return Err("草稿 ID 无效。".into());
        }
        let file = safe_join(&self.root, &format!("drafts/{id}/external/article.md"))?;
        let base = safe_join(&self.root, &format!("drafts/{id}/external/baseline.sha256"))?;
        Ok((file, base))
    }

    fn export_markdown(&self, draft: &Draft) -> Reply<PathBuf> {
        validate(draft, self.posts_dir())?;
        let (file, base) = self.external_paths(&draft.id)?;
        let markdown = crlf(&draft.markdown);
        let stored_base = read_base(&base)?;
        if let Some(external) = read_markdown(&file)? {
            let external_hash = hash(crlf(&external).as_bytes());
            if external_hash == hash(markdown.as_bytes()) {
                atomic_write(&base, external_hash.as_bytes())?;
                return Ok(file);
            }
            // An external editor may have saved changes since the last import.
            if stored_base.as_deref() != Some(external_hash.as_str()) {
                return Ok(file);
            }
        }
        atomic_write(&file, markdown.as_bytes())?;
        atomic_write(&base, hash(markdown.as_bytes()).as_bytes())?;
        Ok(file)
    }

    pub fn open_external_markdown(&self, draft: &Draft) -> Reply<()> {
        let file = self.export_markdown(draft)?;
        open_with_default_app(&file)
    }

    pub fn read_external_markdown(&self, id: &str) -> Reply<Option<ExternalMarkdown>> {
        let (file, base) = self.external_paths(id)?;
        let Some(base_hash) = read_base(&base)? else {
            return Ok(None);
        };
        let Some(draft) = self.storage.get::<Draft>(DRAFTS, id)? else {
            return Ok(None);
        };
        let Some(markdown) = read_markdown(&file)? else {
            return Ok(None);
        };
        let external_hash = hash(crlf(&markdown).as_bytes());
        if external_hash == base_hash {
            return Ok(None);
        }
        if external_hash == hash(crlf(&draft.markdown).as_bytes()) {
            atomic_write(&base, external_hash.as_bytes())?;
            return Ok(None);
        }
        Ok(Some(ExternalMarkdown {
            markdown,
            base_hash,
        }))
    }
}

fn read_base(path: &Path) -> Reply<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(error(err)),
    }
}

fn read_markdown(path: &Path) -> Reply<Option<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(error(err)),
    };
    const LIMIT: u64 = 8 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > LIMIT {
        return Err("外部 Markdown 文件超过 8 MB。".into());
    }
    String::from_utf8(bytes)
        .map(|text| Some(text.trim_start_matches('\u{feff}').to_owned()))
        .map_err(|_| "请在外部应用中将 Markdown 保存为 UTF-8。".into())
}

#[cfg(windows)]
fn open_with_default_app(path: &Path) -> Reply<()> {
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
        0..=32 => Err("无法打开 Markdown 文件。请在 Windows 默认应用中为 .md 设置关联。".into()),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
fn open_with_default_app(_: &Path) -> Reply<()> {
    Err("仅支持在 Windows 中打开默认应用。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_pending_external_edits_when_reopening_and_acknowledges_saved_imports() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("data");
        let workspace =
            Workspace::open(root, &folder.path().join("legacy"), PathBuf::new()).unwrap();
        let initial = "---\ntitle: 中文\n---\n正文\n";
        let mut draft = Draft {
            id: Uuid::new_v4().to_string(),
            path: "content/post/article/index.md".into(),
            original_path: None,
            original_hash: None,
            bundle_origin: None,
            markdown: initial.into(),
            images: vec![],
        };
        workspace.save(&draft).unwrap();
        let file = workspace.export_markdown(&draft).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), crlf(initial));
        assert!(workspace
            .read_external_markdown(&draft.id)
            .unwrap()
            .is_none());
        let incoming = "---\ntitle: 中文\n---\n外部内容\n";
        fs::write(&file, format!("\u{feff}{incoming}")).unwrap();
        draft.markdown.push_str("CMS 改动");
        workspace.save(&draft).unwrap();
        workspace.export_markdown(&draft).unwrap();
        let change = workspace
            .read_external_markdown(&draft.id)
            .unwrap()
            .unwrap();
        assert_eq!(change.markdown, incoming);
        assert_eq!(change.base_hash, hash(crlf(initial).as_bytes()));
        draft.markdown = change.markdown;
        workspace.save(&draft).unwrap();
        assert!(workspace
            .read_external_markdown(&draft.id)
            .unwrap()
            .is_none());
        draft.markdown.push_str("后续改动");
        workspace.export_markdown(&draft).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), crlf(&draft.markdown));
        assert!(workspace.read_external_markdown("../invalid").is_err());
    }
}
