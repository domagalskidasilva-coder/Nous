#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    type Hwnd = *mut core::ffi::c_void;
    type Handle = *mut core::ffi::c_void;
    type Bool = i32;
    type Dword = u32;
    type Lparam = isize;

    const PROCESS_QUERY_LIMITED_INFORMATION: Dword = 0x1000;
    const STILL_ACTIVE: Dword = 259;
    const SW_RESTORE: i32 = 9;

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
        fn IsWindowVisible(hwnd: Hwnd) -> Bool;
        fn ShowWindow(hwnd: Hwnd, cmd: i32) -> Bool;
        fn SetForegroundWindow(hwnd: Hwnd) -> Bool;
        fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
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
            if pid == state.target_pid && IsWindowVisible(hwnd) != 0 {
                state.found = hwnd;
                return 0;
            }
        }
        1
    }

    pub fn focus_pid_window(pid: u32) -> bool {
        let mut state = FindState {
            target_pid: pid,
            found: ptr::null_mut(),
        };
        unsafe {
            EnumWindows(enum_cb, (&mut state as *mut FindState) as Lparam);
            if !state.found.is_null() {
                ShowWindow(state.found, SW_RESTORE);
                SetForegroundWindow(state.found);
                return true;
            }
        }
        false
    }

    pub fn show_error(title: &str, msg: &str) {
        let t = to_wide(title);
        let m = to_wide(msg);
        unsafe {
            MessageBoxW(ptr::null_mut(), m.as_ptr(), t.as_ptr(), 0x10);
        }
    }
}

fn locate_install_paths(exe_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    // 1. Installed standalone layout: <dir>/nous.exe, <dir>/gecko/application.ini, <dir>/runtime/firefox.exe
    let installed_app = exe_dir.join("gecko").join("application.ini");
    let installed_ff = exe_dir.join("runtime").join("firefox.exe");
    if installed_app.is_file() && installed_ff.is_file() {
        return Some((installed_ff, installed_app));
    }

    // 2. Workspace dev layout: target/release/nous-desktop.exe -> ../../gecko & ../../target/nous-gecko-runtime/core/firefox.exe
    let mut cur = Some(exe_dir);
    while let Some(dir) = cur {
        let app = dir.join("gecko").join("application.ini");
        if app.is_file() {
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
                    return Some((ff, app));
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

    let Some((firefox_exe, app_ini)) = locate_install_paths(exe_dir) else {
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
            if !private_mode {
                let _ = fs::write(&pid_file, child.id().to_string());
            }
        }
        Err(err) => {
            #[cfg(target_os = "windows")]
            win::show_error("Nous", &format!("Falha ao iniciar o motor Gecko: {err}"));
        }
    }
}
