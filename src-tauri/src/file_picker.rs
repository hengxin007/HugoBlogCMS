use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, OFN_DONTADDTORECENT, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

pub fn pick_hugo(owner: isize) -> Result<Option<String>, String> {
    let filter: Vec<u16> = "Hugo 可执行文件\0*.exe\0\0".encode_utf16().collect();
    let title: Vec<u16> = "选择 Hugo 可执行文件\0".encode_utf16().collect();
    let mut file = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner as _,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        Flags: OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_DONTADDTORECENT,
        ..Default::default()
    };
    // The filter, title and output buffer all live until the modal call returns.
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let length = file.iter().position(|&ch| ch == 0).unwrap_or(file.len());
        return String::from_utf16(&file[..length])
            .map(Some)
            .map_err(|err| err.to_string());
    }
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        error => Err(format!("无法打开文件选择器（Windows 错误 {error:#x}）。")),
    }
}
