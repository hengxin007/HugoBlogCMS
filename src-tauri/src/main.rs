#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod file_picker;
mod workspace;
use std::{path::PathBuf, sync::Mutex};
use tauri::{
    ipc::Channel,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WebviewWindow, WindowEvent,
};
use workspace::onboarding::{Action, Input, SetupPreview, Startup};
use workspace::{Draft, HugoInfo, Preferences, Workspace};
type Reply<T> = Result<T, String>;
struct Backend(Reply<Mutex<Workspace>>);

#[derive(Default)]
struct TrayQuit(Mutex<Option<Channel<()>>>);

#[tauri::command]
fn register_tray_quit(state: State<'_, TrayQuit>, channel: Channel<()>) -> Reply<()> {
    *state.0.lock().map_err(|_| "托盘退出服务不可用。")? = Some(channel);
    Ok(())
}

#[tauri::command]
fn quit_application(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn show_main_window(window: WebviewWindow) -> Reply<()> {
    window
        .show()
        .and_then(|_| window.unminimize())
        .and_then(|_| window.set_focus())
        .map_err(|error| error.to_string())
}

fn show_from_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = show_main_window(window);
    }
}

fn create_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "tray-show", "显示", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app.default_window_icon().ok_or("找不到应用图标。")?;
    TrayIconBuilder::with_id("blog-cms")
        .icon(icon.clone())
        .tooltip("博客 CMS")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => show_from_tray(app),
            "tray-quit" => {
                let state = app.state::<TrayQuit>();
                let notified = state.0.lock().ok().is_some_and(|registered| {
                    registered
                        .as_ref()
                        .is_some_and(|channel| channel.send(()).is_ok())
                });
                if !notified {
                    app.exit(0);
                }
            }
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_from_tray(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

async fn task<T: Send + 'static>(
    app: AppHandle,
    action: impl FnOnce(&mut Workspace) -> Reply<T> + Send + 'static,
) -> Reply<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Backend>();
        let mut workspace = state
            .0
            .as_ref()
            .map_err(Clone::clone)?
            .lock()
            .map_err(|_| "工作区不可用，请重启应用。")?;
        action(&mut workspace)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn library(app: AppHandle, sync: bool) -> Reply<workspace::Library> {
    task(app, move |ws| ws.library(sync)).await
}
#[tauri::command]
async fn repository_info(app: AppHandle) -> Reply<workspace::repository::RepositoryInfo> {
    task(app, |ws| ws.repository_info()).await
}
#[tauri::command]
async fn refresh_repository(app: AppHandle) -> Reply<workspace::repository::RepositoryRefresh> {
    task(app, |ws| ws.refresh_repository()).await
}
#[tauri::command]
async fn save_draft(app: AppHandle, draft: Draft) -> Reply<()> {
    task(app, move |ws| ws.save(&draft)).await
}
#[tauri::command]
async fn open_external_markdown(app: AppHandle, draft: Draft) -> Reply<()> {
    task(app, move |ws| ws.open_external_markdown(&draft)).await
}
#[tauri::command]
async fn read_external_markdown(
    app: AppHandle,
    id: String,
) -> Reply<Option<workspace::external_edit::ExternalMarkdown>> {
    task(app, move |ws| ws.read_external_markdown(&id)).await
}
#[tauri::command]
async fn insert_image(app: AppHandle, draft: Draft, hosted: bool) -> Reply<workspace::Picture> {
    task(app, move |ws| ws.clipboard_picture(&draft, hosted)).await
}
#[tauri::command]
async fn discard_draft(app: AppHandle, id: String) -> Reply<()> {
    task(app, move |ws| ws.discard(&id)).await
}
#[tauri::command]
async fn duplicate_draft(app: AppHandle, original: Draft, copy: Draft) -> Reply<Draft> {
    task(app, move |ws| ws.duplicate(&original, copy)).await
}
#[tauri::command]
async fn open_article_directory(
    app: AppHandle,
    path: Option<String>,
    draft_id: Option<String>,
) -> Reply<()> {
    task(app, move |ws| {
        ws.open_article_directory(path.as_deref(), draft_id.as_deref())
    })
    .await
}
#[tauri::command]
async fn copy_article_link(app: AppHandle, path: String) -> Reply<String> {
    task(app, move |ws| ws.copy_article_link(&path)).await
}
#[tauri::command]
async fn import_image(
    app: AppHandle,
    draft: Draft,
    hosted: bool,
    png: Vec<u8>,
) -> Reply<workspace::Picture> {
    task(app, move |ws| ws.import_picture(&draft, hosted, png)).await
}
#[tauri::command]
async fn preview_article(app: AppHandle, draft: Draft) -> Reply<String> {
    task(app, move |ws| ws.preview(&draft)).await
}
#[tauri::command]
async fn prepare_publish(
    app: AppHandle,
    draft: Draft,
    release_markdown: String,
) -> Reply<workspace::Summary> {
    let release = draft.with_markdown(release_markdown);
    task(app, move |ws| ws.prepare(&draft, &release)).await
}
#[tauri::command]
async fn publish_article(
    app: AppHandle,
    draft: Draft,
    approval: String,
    release_markdown: String,
) -> Reply<workspace::PublishResult> {
    let release = draft.with_markdown(release_markdown);
    task(app, move |ws| ws.publish(&release, &approval)).await
}
#[tauri::command]
async fn delete_article(app: AppHandle, path: String, hash: String) -> Reply<String> {
    task(app, move |ws| ws.delete_article(&path, &hash)).await
}
#[tauri::command]
async fn settings(app: AppHandle, changes: Option<Preferences>) -> Reply<Preferences> {
    task(app, move |ws| ws.settings(changes)).await
}
#[tauri::command]
async fn inspect_hugo(app: AppHandle, path: String) -> Reply<HugoInfo> {
    task(app, move |ws| ws.inspect_hugo(&path)).await
}
#[tauri::command]
async fn detect_hugo(app: AppHandle) -> Reply<HugoInfo> {
    task(app, |ws| ws.detect_hugo()).await
}
#[tauri::command]
async fn pick_hugo(app: AppHandle) -> Reply<Option<String>> {
    #[cfg(windows)]
    {
        let window = app.get_webview_window("main").ok_or("主窗口尚未就绪。")?;
        let owner = window.hwnd().map_err(|err| err.to_string())?.0 as isize;
        tauri::async_runtime::spawn_blocking(move || file_picker::pick_hugo(owner))
            .await
            .map_err(|err| err.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("请直接填写 Hugo 可执行文件路径。".into())
    }
}
#[tauri::command]
async fn open_preview(app: AppHandle, url: String) -> Reply<()> {
    task(app, move |ws| ws.open_preview(&url)).await
}
#[tauri::command]
async fn open_page(app: AppHandle, page: String, path: Option<String>) -> Reply<()> {
    task(app, move |ws| ws.open_page(&page, path.as_deref())).await
}
#[tauri::command]
async fn data_directory(app: AppHandle) -> Reply<String> {
    task(app, |ws| ws.data_directory()).await
}
#[tauri::command]
async fn startup(app: AppHandle) -> Reply<Startup> {
    task(app, |ws| ws.startup()).await
}
#[tauri::command]
async fn wizard_action(
    app: AppHandle,
    action: Action,
    input: Option<Input>,
    github_token: Option<String>,
    pages_token: Option<String>,
) -> Reply<Startup> {
    task(app, move |ws| {
        ws.wizard_action(action, input, github_token, pages_token)
    })
    .await
}
#[tauri::command]
async fn setup_preview(app: AppHandle) -> Reply<SetupPreview> {
    task(app, |ws| ws.setup_preview()).await
}
#[tauri::command]
async fn setup_browser(app: AppHandle, page: String) -> Reply<()> {
    task(app, move |ws| ws.setup_browser(&page)).await
}
#[tauri::command]
async fn deployment_status(app: AppHandle, commit: String) -> Reply<Option<String>> {
    task(app, move |ws| ws.deployment_status(&commit)).await
}

fn main() {
    let app = tauri::Builder::default()
        .manage(TrayQuit::default())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && window.hide().is_ok() {
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            let legacy = app.path().app_local_data_dir()?;
            let executable = std::env::current_exe()?;
            let root = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).with_file_name("data")
            } else {
                executable.with_file_name("data")
            };
            let sibling = executable.with_file_name("hugo.exe");
            let hugo = if sibling.is_file() || !cfg!(debug_assertions) {
                sibling
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("binaries/hugo-x86_64-pc-windows-msvc.exe")
            };
            let workspace = Workspace::open(root.clone(), &legacy, hugo)
                .map_err(|err| format!("无法打开 CMS 数据目录 {}：\n{err}", root.display()));
            #[cfg(windows)]
            let disable_gpu = workspace.as_ref().ok().is_some_and(Workspace::disable_gpu);
            app.manage(Backend(workspace.map(Mutex::new)));
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|config| config.label == "main")
                .ok_or("未配置主窗口。")?;
            let builder = tauri::WebviewWindowBuilder::from_config(app, window)?
                .initialization_script_for_all_frames(include_str!("preview-frame.js"));
            #[cfg(windows)]
            let builder = if disable_gpu {
                builder
                    .data_directory(legacy.join("WebView2-software"))
                    .additional_browser_args(concat!(
                        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection ",
                        "--autoplay-policy=no-user-gesture-required --disable-gpu"
                    ))
            } else {
                builder
            };
            builder.build()?;
            create_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            register_tray_quit,
            quit_application,
            show_main_window,
            library,
            repository_info,
            refresh_repository,
            save_draft,
            open_external_markdown,
            read_external_markdown,
            insert_image,
            import_image,
            discard_draft,
            duplicate_draft,
            copy_article_link,
            open_article_directory,
            preview_article,
            open_preview,
            prepare_publish,
            publish_article,
            delete_article,
            settings,
            inspect_hugo,
            detect_hugo,
            pick_hugo,
            open_page,
            data_directory,
            startup,
            wizard_action,
            setup_preview,
            setup_browser,
            deployment_status
        ])
        .build(tauri::generate_context!())
        .expect("Hugo CMS startup failed");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let state = handle.state::<Backend>();
            if let Ok(backend) = &state.0 {
                if let Ok(mut workspace) = backend.lock() {
                    workspace.stop_preview();
                }
            }
        }
    });
}
