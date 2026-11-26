#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    type Hwnd = *mut core::ffi::c_void;
    type Handle = *mut core::ffi::c_void;
    type Bool = i32;
    type Dword = u32;
    type Lparam = isize;
    type Wparam = usize;
    type Lresult = isize;

    const PROCESS_QUERY_LIMITED_INFORMATION: Dword = 0x1000;
    const STILL_ACTIVE: Dword = 259;
    const SW_RESTORE: i32 = 9;
    const GW_OWNER: u32 = 4;
    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: Wparam = 0;
    const ICON_BIG: Wparam = 1;
    const IMAGE_ICON: u32 = 1;
    const LR_LOADFROMFILE: u32 = 0x0010;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: Dword, inherit: Bool, pid: Dword) -> Handle;
        fn GetExitCodeProcess(handle: Handle, exit_code: *mut Dword) -> Bool;
        fn CloseHandle(handle: Handle) -> Bool;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            cb: unsafe extern "system" fn(Hwnd, Lparam) -> Bool,
            lparam: Lparam,
        ) -> Bool;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut Dword) -> Dword;
        fn GetWindow(hwnd: Hwnd, cmd: u32) -> Hwnd;
        fn IsWindowVisible(hwnd: Hwnd) -> Bool;
        fn ShowWindow(hwnd: Hwnd, cmd: i32) -> Bool;
        fn SetForegroundWindow(hwnd: Hwnd) -> Bool;
        fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
        fn LoadImageW(
            hinst: Handle,
            name: *const u16,
            itype: u32,
            cx: i32,
            cy: i32,
            fuload: u32,
        ) -> Handle;
        fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
        fn SHGetPropertyStoreForWindow(
            hwnd: Hwnd,
            riid: *const Guid,
            ppv: *mut *mut core::ffi::c_void,
        ) -> i32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut core::ffi::c_void, coinit: u32) -> i32;
        fn CoUninitialize();
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct PropertyKey {
        fmtid: Guid,
        pid: u32,
    }

    #[repr(C)]
    struct PropVariant {
        vt: u16,
        reserved1: u16,
        reserved2: u16,
        reserved3: u16,
        ptr: *const u16,
        pad: usize,
    }

    #[repr(C)]
    struct IPropertyStoreVtbl {
        query_interface: *const core::ffi::c_void,
        add_ref: *const core::ffi::c_void,
        release: unsafe extern "system" fn(*mut IPropertyStore) -> u32,
        get_count: *const core::ffi::c_void,
        get_at: *const core::ffi::c_void,
        get_value: *const core::ffi::c_void,
        set_value: unsafe extern "system" fn(*mut IPropertyStore, *const PropertyKey, *const PropVariant) -> i32,
        commit: unsafe extern "system" fn(*mut IPropertyStore) -> i32,
    }

    #[repr(C)]
    struct IPropertyStore {
        vtbl: *const IPropertyStoreVtbl,
    }

    const IID_IPROPERTYSTORE: Guid = Guid {
        data1: 0x886D_8EEB,
        data2: 0x8CF2,
        data3: 0x4446,
        data4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99],
    };

    const FMTID_APPUSERMODEL: Guid = Guid {
        data1: 0x9F4C_2855,
        data2: 0x9F79,
        data3: 0x4B39,
        data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
    };

    const VT_LPWSTR: u16 = 31;
    const COINIT_APARTMENTTHREADED: u32 = 2;

    fn set_prop_str(store: *mut IPropertyStore, pid: u32, val: &str) {
        let wide = to_wide(val);
        let key = PropertyKey {
            fmtid: FMTID_APPUSERMODEL,
            pid,
        };
        let pv = PropVariant {
            vt: VT_LPWSTR,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            ptr: wide.as_ptr(),
            pad: 0,
        };
        unsafe {
            let _ = ((*(*store).vtbl).set_value)(store, &key, &pv);
        }
    }

    pub fn apply_taskbar_identity(hwnd: Hwnd, app_id: &str, exe_path: &Path, ico_path: Option<&Path>) {
        unsafe {
            let hr_init = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let mut raw: *mut core::ffi::c_void = ptr::null_mut();
            let hr = SHGetPropertyStoreForWindow(hwnd, &IID_IPROPERTYSTORE, &mut raw);
            if hr >= 0 && !raw.is_null() {
                let store = raw as *mut IPropertyStore;
                set_prop_str(store, 5, app_id); // PKEY_AppUserModel_ID
                set_prop_str(store, 4, "Nous"); // PKEY_AppUserModel_RelaunchDisplayNameResource
                if let Some(exe_str) = exe_path.to_str() {
                    set_prop_str(store, 2, &format!("\"{exe_str}\"")); // PKEY_AppUserModel_RelaunchCommand
                }
                if let Some(ico) = ico_path
                    && let Some(ico_str) = ico.to_str()
                {
                    set_prop_str(store, 3, &format!("{ico_str},0")); // PKEY_AppUserModel_RelaunchIconResource
                }
                let _ = ((*(*store).vtbl).commit)(store);
                let _ = ((*(*store).vtbl).release)(store);
            }
            if hr_init >= 0 {
                CoUninitialize();
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn set_app_user_model_id(id: &str) {
        let wide = to_wide(id);
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
        }
    }

    pub fn is_pid_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code: Dword = 0;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE
        }
    }

    struct FindState {
        target_pid: u32,
        found: Hwnd,
    }

    unsafe extern "system" fn enum_cb(hwnd: Hwnd, lparam: Lparam) -> Bool {
        let state = unsafe { &mut *(lparam as *mut FindState) };
        let mut pid: Dword = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == state.target_pid
                && IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
            {
                state.found = hwnd;
                return 0;
            }
        }
        1
    }

    pub fn find_main_window(pid: u32) -> Option<Hwnd> {
        let mut state = FindState {
            target_pid: pid,
            found: ptr::null_mut(),
        };
        unsafe {
            EnumWindows(enum_cb, (&mut state as *mut FindState) as Lparam);
        }
        if state.found.is_null() {
            None
        } else {
            Some(state.found)
        }
    }

    pub fn focus_pid_window(pid: u32) -> bool {
        if let Some(hwnd) = find_main_window(pid) {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            return true;
        }
        false
    }

    pub fn apply_window_icon(hwnd: Hwnd, ico_path: &Path) {
        let Some(s) = ico_path.to_str() else {
            return;
        };
        let wide = to_wide(s);
        unsafe {
            let h_big = LoadImageW(ptr::null_mut(), wide.as_ptr(), IMAGE_ICON, 32, 32, LR_LOADFROMFILE);
            if !h_big.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG, h_big as Lparam);
            }
            let h_small = LoadImageW(ptr::null_mut(), wide.as_ptr(), IMAGE_ICON, 16, 16, LR_LOADFROMFILE);
            if !h_small.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL, h_small as Lparam);
            }
        }
    }

    pub fn show_error(title: &str, msg: &str) {
        let t = to_wide(title);
        let m = to_wide(msg);
        unsafe {
            MessageBoxW(ptr::null_mut(), m.as_ptr(), t.as_ptr(), 0x10);
        }
    }
}

fn locate_install_paths(exe_dir: &Path) -> Option<(PathBuf, PathBuf, Option<PathBuf>)> {
    // 1. Installed standalone layout:
    //    <dir>/nous.exe, <dir>/runtime/firefox.exe, <dir>/runtime/nous-app/application.ini
    //    Keeping nous-app inside runtime/ ensures Gecko's Windows content sandbox always allows
    //    remote content processes to load NousContentChild.sys.mjs under full sandboxing.
    let installed_ff = exe_dir.join("runtime").join("firefox.exe");
    let installed_ico = exe_dir.join("nous.ico");
    let sandbox_safe_app = exe_dir.join("runtime").join("nous-app").join("application.ini");
    let legacy_installed_app = exe_dir.join("gecko").join("application.ini");
    if installed_ff.is_file() {
        if sandbox_safe_app.is_file() {
            return Some((
                installed_ff,
                sandbox_safe_app,
                installed_ico.is_file().then_some(installed_ico),
            ));
        }
        if legacy_installed_app.is_file() {
            return Some((
                installed_ff,
                legacy_installed_app,
                installed_ico.is_file().then_some(installed_ico),
            ));
        }
    }

    // 2. Workspace dev layout
    let mut cur = Some(exe_dir);
    while let Some(dir) = cur {
        let app = dir.join("gecko").join("application.ini");
        if app.is_file() {
            let ico = dir.join("assets").join("nous.ico");
            let candidates = [
                dir.join("target")
                    .join("nous-gecko-runtime")
                    .join("core")
                    .join("firefox.exe"),
                PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe"),
            ];
            for ff in candidates {
                if ff.is_file() {
                    return Some((ff, app, ico.is_file().then_some(ico)));
                }
            }
        }
        cur = dir.parent();
    }

    None
}

fn default_profile_dir() -> PathBuf {
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Nous").join("Profile");
    }
    env::temp_dir().join("NousProfile")
}

fn normalize_url_arg(arg: &str) -> String {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let path = Path::new(trimmed);
    if path.exists()
        && let Ok(canon) = fs::canonicalize(path)
    {
        let s = canon.to_string_lossy().replace("\\\\?\\", "").replace('\\', "/");
        return format!("file:///{s}");
    }
    trimmed.to_string()
}

fn main() {
    #[cfg(target_os = "windows")]
    win::set_app_user_model_id("Nous.Browser.Desktop");

    let exe_path = env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));

    let Some((firefox_exe, app_ini, ico_path)) = locate_install_paths(exe_dir) else {
        #[cfg(target_os = "windows")]
        win::show_error(
            "Nous",
            "Runtime Gecko (firefox.exe) ou gecko/application.ini não encontrado ao lado do executável.",
        );
        return;
    };

    let mut url = String::new();
    let mut private_mode = false;
    let mut clean_profile = false;
    let mut custom_profile: Option<PathBuf> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--private" | "-private" | "--incognito" => private_mode = true,
            "--clean" => clean_profile = true,
            "--profile" => {
                if let Some(p) = args.next() {
                    custom_profile = Some(PathBuf::from(p));
                }
            }
            s if s.starts_with('-') => {}
            other => {
                if url.is_empty() {
                    url = normalize_url_arg(other);
                }
            }
        }
    }

    let profile_dir = custom_profile.unwrap_or_else(default_profile_dir);
    if clean_profile && profile_dir.exists() {
        let _ = fs::remove_dir_all(&profile_dir);
    }
    let _ = fs::create_dir_all(&profile_dir);

    // Single-instance handoff: if Nous is already running on this profile, forward URL & focus it.
    let pid_file = profile_dir.join("nous.pid");
    #[cfg(target_os = "windows")]
    if !private_mode
        && !clean_profile
        && let Ok(raw_pid) = fs::read_to_string(&pid_file)
        && let Ok(pid) = raw_pid.trim().parse::<u32>()
        && win::is_pid_alive(pid)
    {
        if !url.is_empty() {
            let queue_file = profile_dir.join("open-url.queue");
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(queue_file) {
                let _ = writeln!(f, "{url}");
            }
        }
        if win::focus_pid_window(pid) {
            return;
        }
    }

    let mut cmd = Command::new(&firefox_exe);
    cmd.arg("-app")
        .arg(&app_ini)
        .arg("-no-remote")
        .arg("-profile")
        .arg(&profile_dir)
        .env_remove("XUL_APP_FILE")
        .env_remove("MOZ_DISABLE_CONTENT_SANDBOX")
        .env("NOUS_GECKO_START_URL", &url)
        .env("MOZ_CRASHREPORTER_DISABLE", "1")
        .env("MOZ_DISABLE_AUTO_SAFE_MODE", "1");

    if private_mode {
        cmd.env("NOUS_PRIVATE_WINDOW", "1");
    } else {
        cmd.env_remove("NOUS_PRIVATE_WINDOW");
    }

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            if !private_mode {
                let _ = fs::write(&pid_file, pid.to_string());
            }
            // Wait briefly for the visible browser HWND and stamp Nous taskbar identity & icon.
            #[cfg(target_os = "windows")]
            {
                for _ in 0..75 {
                    if let Some(hwnd) = win::find_main_window(pid) {
                        win::apply_taskbar_identity(
                            hwnd,
                            "Nous.Browser.Desktop",
                            &exe_path,
                            ico_path.as_deref(),
                        );
                        if let Some(ref ico) = ico_path {
                            win::apply_window_icon(hwnd, ico);
                        }
                        break;
                    }
                    if !win::is_pid_alive(pid) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(80));
                }
            }
        }
        Err(err) => {
            #[cfg(target_os = "windows")]
            win::show_error("Nous", &format!("Falha ao iniciar o motor Gecko: {err}"));
        }
    }
}
