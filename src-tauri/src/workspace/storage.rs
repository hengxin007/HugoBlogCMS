use super::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;

pub(super) const SETTINGS: &str = "settings";
pub(super) const DRAFTS: &str = "drafts";
pub(super) const OPERATIONS: &str = "operations";
const DATABASE: &str = "cms.sqlite3";

pub(super) struct Storage(Connection);

impl Storage {
    pub(super) fn open(root: &Path) -> Reply<Self> {
        let connection = Connection::open(root.join(DATABASE))
            .map_err(|err| format!("无法打开数据目录中的 SQLite 数据库：{err}"))?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS records (
                    category TEXT NOT NULL,
                    id TEXT NOT NULL,
                    document TEXT NOT NULL,
                    PRIMARY KEY (category, id)
                ) WITHOUT ROWID;",
            )
            .map_err(error)?;
        Ok(Self(connection))
    }

    pub(super) fn get<T: DeserializeOwned>(&self, category: &str, id: &str) -> Reply<Option<T>> {
        let document: Option<String> = self
            .0
            .query_row(
                "SELECT document FROM records WHERE category = ?1 AND id = ?2",
                params![category, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        document
            .map(|text| serde_json::from_str(&text).map_err(error))
            .transpose()
    }

    pub(super) fn put(&self, category: &str, id: &str, data: &impl Serialize) -> Reply<()> {
        let document = serde_json::to_string(data).map_err(error)?;
        self.0
            .execute(
                "INSERT INTO records (category, id, document) VALUES (?1, ?2, ?3)
             ON CONFLICT (category, id) DO UPDATE SET document = excluded.document",
                params![category, id, document],
            )
            .map_err(error)?;
        Ok(())
    }

    pub(super) fn remove(&self, category: &str, id: &str) -> Reply<()> {
        self.0
            .execute(
                "DELETE FROM records WHERE category = ?1 AND id = ?2",
                params![category, id],
            )
            .map_err(error)?;
        Ok(())
    }

    pub(super) fn list<T: DeserializeOwned>(&self, category: &str) -> Reply<Vec<T>> {
        let mut query = self
            .0
            .prepare("SELECT document FROM records WHERE category = ?1 ORDER BY id")
            .map_err(error)?;
        let rows = query
            .query_map([category], |row| row.get::<_, String>(0))
            .map_err(error)?;
        rows.map(|row| serde_json::from_str(&row.map_err(error)?).map_err(error))
            .collect()
    }
}

pub(super) fn migrate(root: &Path, legacy: &Path) -> Reply<()> {
    if root.exists() || !legacy.is_dir() {
        return Ok(());
    }
    let staged =
        tempfile::tempdir_in(root.parent().ok_or("数据目录没有上级目录。")?).map_err(error)?;
    {
        let database = Storage::open(staged.path())?;
        for (file, id) in [
            ("preferences.json", "preferences"),
            ("proxy.json", "proxy"),
            ("site.json", "site"),
            ("onboarding.json", "onboarding"),
            ("welcome-acknowledged.json", "welcome-acknowledged"),
        ] {
            if let Some(value) = read_json::<serde_json::Value>(&legacy.join(file))? {
                database.put(SETTINGS, id, &value)?;
            }
        }
        for dir in ["source", "drafts", "onboarding", "discarded-drafts"] {
            let source = legacy.join(dir);
            if source.is_dir() {
                copy_tree(&source, &staged.path().join(dir))?;
            }
        }
        let drafts = staged.path().join("drafts");
        if drafts.is_dir() {
            for entry in fs::read_dir(drafts).map_err(error)? {
                let entry = entry.map_err(error)?;
                let name = entry.file_name();
                let id = name.to_string_lossy();
                if uuid(&id) {
                    let file = entry.path().join("article.json");
                    if let Some(draft) = read_json::<Draft>(&file)? {
                        database.put(DRAFTS, &id, &draft)?;
                        fs::remove_file(file).map_err(error)?;
                    }
                }
            }
        }
        let journals = legacy.join("operations");
        if journals.is_dir() {
            for entry in fs::read_dir(journals).map_err(error)? {
                let file = entry.map_err(error)?.path();
                if let Some(id) = file
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|_| file.extension().is_some_and(|ext| ext == "json"))
                {
                    if let Some(journal) = read_json::<Journal>(&file)? {
                        database.put(OPERATIONS, id, &journal)?;
                    }
                }
            }
        }
    }
    // The old directory remains intact. Only a complete migration becomes visible.
    fs::rename(staged.path(), root).map_err(error)
}

fn copy_tree(source: &Path, destination: &Path) -> Reply<()> {
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(error)?;
        let target = destination.join(entry.path().strip_prefix(source).map_err(error)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target).map_err(error)?;
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), target).map_err(error)?;
        } else {
            return Err(format!(
                "数据迁移遇到不支持的文件类型：{}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_data_and_restores_it_from_a_copied_directory() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let legacy = root.join("old-app-data");
        let data = root.join("data");
        let restored = root.join("restored/data");
        let draft = Draft {
            id: Uuid::new_v4().to_string(),
            path: "content/post/first/index.md".into(),
            original_path: None,
            original_hash: None,
            bundle_origin: None,
            markdown: "---\r\ntitle: 保留草稿\r\n---\r\n正文\r\n".into(),
            images: vec![],
        };
        atomic_write(
            &legacy.join("preferences.json"),
            br#"{"theme":"dark","editorFontSize":18}"#,
        )
        .unwrap();
        atomic_write(&legacy.join("welcome-acknowledged.json"), b"true").unwrap();
        atomic_write(
            &legacy.join("operations/first.json"),
            br#"{"commit":"legacy-sha"}"#,
        )
        .unwrap();
        let metadata = legacy.join("drafts").join(&draft.id).join("article.json");
        atomic_write(&metadata, &serde_json::to_vec(&draft).unwrap()).unwrap();
        let picture = format!("drafts/{}/images/example.png", draft.id);
        atomic_write(&legacy.join(&picture), b"image").unwrap();
        atomic_write(
            &legacy.join("source/content/post/first/index.md"),
            b"source",
        )
        .unwrap();

        let workspace = Workspace::open(data.clone(), &legacy, PathBuf::new()).unwrap();
        assert_eq!(workspace.preferences.editor_font_size, 18);
        assert!(!workspace
            .storage
            .get::<Draft>(DRAFTS, &draft.id)
            .unwrap()
            .unwrap()
            .markdown
            .is_empty());
        assert!(metadata.is_file());
        assert!(!data
            .join("drafts")
            .join(&draft.id)
            .join("article.json")
            .exists());
        assert_eq!(fs::read(data.join(&picture)).unwrap(), b"image");
        assert_eq!(
            workspace
                .storage
                .get::<Journal>(OPERATIONS, "first")
                .unwrap()
                .unwrap()
                .commit
                .as_deref(),
            Some("legacy-sha")
        );
        workspace
            .storage
            .put(SETTINGS, "preferences", &Preferences::default())
            .unwrap();
        drop(workspace);

        copy_tree(&data, &restored).unwrap();
        let workspace = Workspace::open(restored.clone(), &legacy, PathBuf::new()).unwrap();
        assert_eq!(
            workspace.preferences.editor_font_size,
            Preferences::default().editor_font_size
        );
        assert_eq!(
            workspace.storage.list::<Draft>(DRAFTS).unwrap()[0].markdown,
            draft.markdown
        );
        assert_eq!(fs::read(restored.join(&picture)).unwrap(), b"image");
        assert!(restored
            .join("source/content/post/first/index.md")
            .is_file());
    }
}
