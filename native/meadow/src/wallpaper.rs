//! Wallpaper mode (Windows): the meadow lives behind the desktop icons, pauses while a maximised
//! or fullscreen window hides the desktop, and a tray icon offers autostart and exit.
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::BOOL;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use winit::window::{Window, WindowAttributes};

pub const CHECK_EVERY: Duration = Duration::from_secs(1);
// At login Explorer may not have built the desktop yet; after this long we give up and open a
// normal window instead.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "pixel3d meadow";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// True if another wallpaper instance runs. The mutex stays open for this process's lifetime.
pub fn already_running() -> bool {
    unsafe {
        let mutex = CreateMutexW(null(), 0, wide("Local\\pixel3d-meadow-wallpaper").as_ptr());
        !mutex.is_null() && GetLastError() == ERROR_ALREADY_EXISTS
    }
}

pub struct Desktop {
    host: HWND, // our window's parent behind the icons; null until attached
    tray: Option<(TrayIcon, CheckMenuItem, MenuItem)>, // icon, autostart, exit
    since: Instant, // when we started looking for the desktop
    pub next_check: Instant,
    pub fallback: bool, // gave up on the desktop: a normal window
}

impl Desktop {
    pub fn new() -> Self {
        let now = Instant::now();
        Desktop { host: null_mut(), tray: None, since: now, next_check: now, fallback: false }
    }

    /// Call once the event loop runs (the tray needs its message pump).
    pub fn show_tray(&mut self) {
        if self.tray.is_some() {
            return;
        }
        let autostart = CheckMenuItem::new("Chạy cùng Windows", true, autostart_enabled(), None);
        let exit = MenuItem::new("Thoát", true, None);
        let menu = Menu::new();
        menu.append_items(&[&autostart, &exit]).expect("tray menu");
        let icon = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("pixel3d meadow").with_icon(icon()).build().expect("tray icon");
        self.tray = Some((icon, autostart, exit));
    }

    /// Handles a tray menu click; true means exit.
    pub fn menu(&self, event: &MenuEvent) -> bool {
        let Some((_, autostart, exit)) = &self.tray else { return false };
        if event.id == *autostart.id() {
            set_autostart(autostart.is_checked());
        }
        event.id == *exit.id()
    }

    /// Our window was destroyed (Explorer restarted): look for the desktop afresh.
    pub fn lost(&mut self) {
        self.host = null_mut();
        self.since = Instant::now();
    }

    /// Attributes for a window behind the desktop icons, or None while there's no desktop yet;
    /// past ATTACH_TIMEOUT this switches to `fallback` (a normal window) and logs why.
    pub fn window_attributes(&mut self) -> Option<WindowAttributes> {
        if self.host.is_null() || unsafe { IsWindow(self.host) } == 0 {
            self.host = create_host().unwrap_or(null_mut());
        }
        if self.host.is_null() {
            if self.since.elapsed() > ATTACH_TIMEOUT {
                log::warn!("no desktop to attach to after {}s: running as a normal window", ATTACH_TIMEOUT.as_secs());
                self.fallback = true;
            }
            return None;
        }
        log::info!("attached behind the desktop icons");
        let (w, h) = screen_size();
        let parent = RawWindowHandle::Win32(Win32WindowHandle::new((self.host as isize).try_into().expect("host hwnd")));
        let attrs = Window::default_attributes()
            .with_title("meadow")
            .with_decorations(false)
            .with_position(PhysicalPosition::new(0, 0))
            .with_inner_size(PhysicalSize::new(w as u32, h as u32));
        // SAFETY: the host is a live window of this thread (checked just above).
        Some(unsafe { attrs.with_parent_window(Some(parent)) })
    }
}

fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Makes the screen-sized window our window goes into, behind the desktop icons.
fn create_host() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), null());
        if progman.is_null() {
            return None;
        }
        // Asks Explorer for a separate wallpaper layer (0xD, 1 as Lively sends; send it only once).
        let mut result = 0;
        SendMessageTimeoutW(progman, 0x052C, 0xD, 1, SMTO_NORMAL, 1000, &mut result);
        let icons_class = wide("SHELLDLL_DefView");
        // Windows 11 24H2 "raised desktop": the icons and the wallpaper layer are both Progman's
        // children; we go between them as an opaque layered child (Microsoft's advice, as Lively).
        let raised = GetWindowLongPtrW(progman, GWL_EXSTYLE) & WS_EX_NOREDIRECTIONBITMAP as isize != 0;
        let (parent, below, ex_style) = if raised {
            let icons = FindWindowExW(progman, null_mut(), icons_class.as_ptr(), null());
            if icons.is_null() {
                return None;
            }
            (progman, icons, WS_EX_LAYERED)
        } else {
            // Older layout: the top-level WorkerW right behind the one holding the icons.
            let mut icons_holder: HWND = null_mut();
            EnumWindows(Some(find_icons_holder), &mut icons_holder as *mut HWND as LPARAM);
            if icons_holder.is_null() {
                return None;
            }
            let worker = FindWindowExW(null_mut(), icons_holder, wide("WorkerW").as_ptr(), null());
            if worker.is_null() {
                return None;
            }
            (worker, HWND_TOP, 0)
        };
        let class = wide("pixel3d-desktop");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            hbrBackground: GetStockObject(BLACK_BRUSH),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when already registered
        let mut origin = POINT { x: 0, y: 0 };
        ScreenToClient(parent, &mut origin);
        let (w, h) = screen_size();
        let host = CreateWindowExW(ex_style, class.as_ptr(), null(), WS_CHILD | WS_VISIBLE, origin.x, origin.y, w, h, parent, null_mut(), instance, null());
        if host.is_null() {
            log::warn!("desktop host window: error {}", GetLastError());
            return None;
        }
        if raised {
            SetLayeredWindowAttributes(host, 0, 255, LWA_ALPHA);
        }
        SetWindowPos(host, below, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        log::info!("desktop layout: {}", if raised { "raised (24H2)" } else { "classic WorkerW" });
        Some(host)
    }
}

unsafe extern "system" fn find_icons_holder(hwnd: HWND, out: LPARAM) -> BOOL {
    unsafe {
        if FindWindowExW(hwnd, null_mut(), wide("SHELLDLL_DefView").as_ptr(), null()).is_null() {
            return 1;
        }
        *(out as *mut HWND) = hwnd;
        0
    }
}

/// True when a maximised window, or a fullscreen foreground one, hides the desktop.
pub fn desktop_covered() -> bool {
    let mut covered = false;
    unsafe { EnumWindows(Some(check_covering), &mut covered as *mut bool as LPARAM) };
    covered
}

unsafe extern "system" fn check_covering(hwnd: HWND, out: LPARAM) -> BOOL {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || cloaked(hwnd) || is_shell(hwnd) {
            return 1;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut r);
        let (w, h) = screen_size();
        let fullscreen = hwnd == GetForegroundWindow() && r.left <= 0 && r.top <= 0 && r.right >= w && r.bottom >= h;
        if IsZoomed(hwnd) != 0 || fullscreen {
            *(out as *mut bool) = true;
            return 0;
        }
        1
    }
}

// Windows on another virtual desktop, or suspended UWP apps, are "cloaked": visible but not shown.
fn cloaked(hwnd: HWND) -> bool {
    let mut value = 0u32;
    let hr = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut value as *mut u32 as _, 4) };
    hr == 0 && value != 0
}

fn is_shell(hwnd: HWND) -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

fn autostart_enabled() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), null_mut()) == 0 }
}

/// Adds/removes the HKCU Run entry: this exe with the current arguments (so --wallpaper too).
fn set_autostart(on: bool) {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    let err = if on {
        let exe = std::env::current_exe().expect("exe path");
        let args: Vec<String> = std::env::args().skip(1).collect();
        let command = wide(&format!("\"{}\" {}", exe.display(), args.join(" ")));
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), REG_SZ, command.as_ptr() as _, (command.len() * 2) as u32) }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) }
    };
    log::info!("autostart {}: {}", if on { "on" } else { "off" }, if err == 0 { "ok".into() } else { format!("error {err}") });
}

// 32×32: sky, sun, grass.
fn icon() -> Icon {
    const N: u32 = 32;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let [r, g, b] = if y >= 20 {
                [0x58, 0x9e, 0x63]
            } else if (x - 22).pow(2) + (y - 9).pow(2) < 30 {
                [0xff, 0xf1, 0xd6]
            } else {
                [0x7f, 0xb2, 0xe5]
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("tray icon image")
}
