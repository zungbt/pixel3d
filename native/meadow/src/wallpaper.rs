//! Wallpaper mode (Windows): the meadow lives behind the desktop icons, a window per monitor (or
//! the primary's only), each pausing while a maximised or fullscreen window hides it, and a tray
//! icon offers the settings, the city search, autostart and exit.
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use meadow_core::frame_pacer::FPS_CAPS;
use meadow_core::settings::{PIXEL_SIZES, Setting, Settings};
use meadow_core::sky::Fog;
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    BLACK_BRUSH, EnumDisplayMonitors, GetMonitorInfoW, GetStockObject, HDC, HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO,
    MonitorFromWindow, ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetProcessInformation,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::BOOL;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use winit::window::{Window, WindowAttributes};

use crate::render::Screen;

pub const CHECK_EVERY: Duration = Duration::from_secs(1);
pub const PAUSE_AFTER: Duration = Duration::from_secs(15); // a screen hidden this long stops drawing
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

pub enum TrayAction {
    Exit,
    Set(Setting),
    City, // open the city search
}

struct Tray {
    _icon: TrayIcon,
    choices: Vec<(CheckMenuItem, Setting)>, // one checked per kind of setting, like radio buttons
    city: MenuItem,
    show_clock: CheckMenuItem,
    autostart: CheckMenuItem,
    exit: MenuItem,
}

/// A monitor's handle and rectangle (physical px: the process is per-monitor DPI aware).
#[derive(Clone, Copy)]
pub struct Monitor {
    handle: HMONITOR,
    rect: RECT,
}

impl PartialEq for Monitor {
    fn eq(&self, o: &Self) -> bool {
        let (a, b) = (self.rect, o.rect);
        self.handle == o.handle && (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
    }
}

impl std::fmt::Debug for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let r = self.rect;
        write!(f, "{}x{}@{},{}", r.right - r.left, r.bottom - r.top, r.left, r.top)
    }
}

/// Every monitor, the primary first, then top to bottom and left to right.
pub fn monitors() -> Vec<Monitor> {
    let mut found: Vec<(bool, Monitor)> = Vec::new();
    unsafe { EnumDisplayMonitors(null_mut(), null(), Some(add_monitor), &mut found as *mut _ as LPARAM) };
    found.sort_by_key(|(primary, m)| (!primary, m.rect.top, m.rect.left));
    found.into_iter().map(|(_, m)| m).collect()
}

unsafe extern "system" fn add_monitor(handle: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
    unsafe {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        if GetMonitorInfoW(handle, &mut info) != 0 {
            (*(out as *mut Vec<(bool, Monitor)>)).push((info.dwFlags & MONITORINFOF_PRIMARY != 0, Monitor { handle, rect: info.rcMonitor }));
        }
        1
    }
}

pub struct Desktop {
    host: HWND, // our windows' parent behind the icons, over the monitors they cover; null until attached
    monitors: Vec<Monitor>, // the ones our windows cover, the primary first
    tray: Option<Tray>,
    since: Instant, // when we started looking for the desktop
    pub next_check: Instant,
    pub fallback: bool, // gave up on the desktop: a normal window
}

impl Desktop {
    pub fn new() -> Self {
        let now = Instant::now();
        Desktop { host: null_mut(), monitors: Vec::new(), tray: None, since: now, next_check: now, fallback: false }
    }

    /// Call once the event loop runs (the tray needs its message pump).
    pub fn show_tray(&mut self, settings: &Settings, city: Option<&str>) {
        if self.tray.is_some() {
            return;
        }
        let fps = |cap: Option<u32>| (cap.map_or("Không giới hạn".into(), |n| format!("{n} fps")), Setting::Fps(cap));
        let groups: [(&str, Vec<(String, Setting)>); 6] = [
            ("Cỡ điểm ảnh", PIXEL_SIZES.iter().map(|&n| (format!("{n} px"), Setting::PixelSize(n))).collect()),
            ("Giới hạn FPS", FPS_CAPS.iter().map(|&n| fps(Some(n))).chain([fps(None)]).collect()),
            ("Thời gian", vec![("Theo đồng hồ".into(), Setting::Clock(true)), ("Ngày 5 phút".into(), Setting::Clock(false))]),
            ("Giờ đồng hồ", vec![("Giờ máy tính".into(), Setting::CityTime(false)), ("Giờ thành phố".into(), Setting::CityTime(true))]),
            ("Sương mù", [("Tắt", Fog::Off), ("Tự động", Fog::Auto), ("Luôn bật", Fog::Always)].map(|(l, f)| (l.into(), Setting::Fog(f))).to_vec()),
            ("Màn hình", vec![("Tất cả màn hình".into(), Setting::AllScreens(true)), ("Chỉ màn hình chính".into(), Setting::AllScreens(false))]),
        ];
        let menu = Menu::new();
        let mut choices = Vec::new();
        for (title, group) in groups {
            // Checked if choosing it would change nothing.
            let items: Vec<_> = group
                .into_iter()
                .map(|(label, setting)| {
                    let mut chosen = *settings;
                    chosen.set(setting);
                    (CheckMenuItem::new(label, true, chosen == *settings, None), setting)
                })
                .collect();
            let refs: Vec<&dyn IsMenuItem> = items.iter().map(|(item, _)| item as &dyn IsMenuItem).collect();
            menu.append(&Submenu::with_items(title, true, &refs).expect("tray submenu")).expect("tray menu");
            choices.extend(items);
        }
        let city = MenuItem::new(city_label(city), true, None);
        let show_clock = CheckMenuItem::new("Hiện đồng hồ", true, settings.show_clock, None);
        let autostart = CheckMenuItem::new("Chạy cùng Windows", true, autostart_enabled(), None);
        let exit = MenuItem::new("Thoát", true, None);
        menu.append_items(&[&city, &show_clock, &PredefinedMenuItem::separator(), &autostart, &exit]).expect("tray menu");
        let icon = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("pixel3d meadow").with_icon(icon()).build().expect("tray icon");
        self.tray = Some(Tray { _icon: icon, choices, city, show_clock, autostart, exit });
    }

    pub fn set_city(&self, name: &str) {
        if let Some(tray) = &self.tray {
            tray.city.set_text(city_label(Some(name)));
        }
    }

    /// Handles a tray menu click.
    pub fn menu(&self, event: &MenuEvent) -> Option<TrayAction> {
        let tray = self.tray.as_ref()?;
        if event.id == *tray.autostart.id() {
            set_autostart(tray.autostart.is_checked());
            return None;
        }
        if event.id == *tray.exit.id() {
            return Some(TrayAction::Exit);
        }
        if event.id == *tray.city.id() {
            return Some(TrayAction::City);
        }
        if event.id == *tray.show_clock.id() {
            return Some(TrayAction::Set(Setting::ShowClock(tray.show_clock.is_checked())));
        }
        // A click toggles its check: put the whole group right, this one checked even if it was.
        let &(_, chosen) = tray.choices.iter().find(|(item, _)| event.id == *item.id())?;
        for (item, setting) in &tray.choices {
            if std::mem::discriminant(setting) == std::mem::discriminant(&chosen) {
                item.set_checked(*setting == chosen);
            }
        }
        Some(TrayAction::Set(chosen))
    }

    /// False once the desktop (and our window with it) is gone.
    pub fn attached(&self) -> bool {
        unsafe { IsWindow(self.host) != 0 }
    }

    /// Our windows were destroyed (Explorer restarted), or the monitors changed and they were
    /// closed: take the host down too and look for the desktop afresh.
    pub fn lost(&mut self) {
        if !self.host.is_null() {
            unsafe { DestroyWindow(self.host) }; // fails harmlessly if Explorer took it already
        }
        self.host = null_mut();
        self.since = Instant::now();
    }

    /// True when monitors came or went or moved since our windows were opened.
    pub fn monitors_changed(&self, all_screens: bool) -> bool {
        let now = monitors();
        self.monitors != if all_screens { now } else { now.into_iter().take(1).collect() }
    }

    /// Which of our monitors a maximised or fullscreen window hides, in our windows' order.
    pub fn covered(&self) -> Vec<bool> {
        let mut covered = (self.monitors.clone(), vec![false; self.monitors.len()]);
        unsafe { EnumWindows(Some(check_covering), &mut covered as *mut _ as LPARAM) };
        covered.1
    }

    /// A window behind the desktop icons for each monitor (only the primary, unless `all_screens`)
    /// and where it sits, or None while there's no desktop yet; past ATTACH_TIMEOUT this switches
    /// to `fallback` (a normal window) and logs why.
    pub fn window_attributes(&mut self, all_screens: bool) -> Option<Vec<(WindowAttributes, Screen)>> {
        if self.host.is_null() || unsafe { IsWindow(self.host) } == 0 {
            self.monitors = monitors();
            self.monitors.truncate(if all_screens { self.monitors.len() } else { 1 });
            self.host = create_host(bounds(&self.monitors)).unwrap_or(null_mut());
        }
        if self.host.is_null() {
            if self.since.elapsed() > ATTACH_TIMEOUT {
                log::warn!("no desktop to attach to after {}s: running as a normal window", ATTACH_TIMEOUT.as_secs());
                self.fallback = true;
            }
            return None;
        }
        log::info!("attached behind the desktop icons on {:?}", self.monitors);
        let host = bounds(&self.monitors);
        let primary = self.monitors[0].rect;
        let parent = RawWindowHandle::Win32(Win32WindowHandle::new((self.host as isize).try_into().expect("host hwnd")));
        let windows = self.monitors.iter().map(|m| {
            let r = m.rect;
            let size = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
            let attrs = Window::default_attributes()
                .with_title("meadow")
                .with_decorations(false)
                .with_position(PhysicalPosition::new(r.left - host.left, r.top - host.top))
                .with_inner_size(PhysicalSize::new(size.0, size.1));
            // SAFETY: the host is a live window of this thread (checked just above).
            let attrs = unsafe { attrs.with_parent_window(Some(parent)) };
            (attrs, Screen { offset: (r.left - primary.left, r.top - primary.top), size })
        });
        Some(windows.collect())
    }
}

/// The rectangle around these monitors.
fn city_label(name: Option<&str>) -> String {
    name.map_or("Thành phố…".into(), |n| format!("Thành phố… ({n})"))
}

fn bounds(monitors: &[Monitor]) -> RECT {
    monitors.iter().fold(monitors[0].rect, |a, m| RECT {
        left: a.left.min(m.rect.left),
        top: a.top.min(m.rect.top),
        right: a.right.max(m.rect.right),
        bottom: a.bottom.max(m.rect.bottom),
    })
}

/// Makes the window over `area` (screen px) that our windows go into, behind the desktop icons.
fn create_host(area: RECT) -> Option<HWND> {
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
        let mut origin = POINT { x: area.left, y: area.top };
        ScreenToClient(parent, &mut origin);
        let (w, h) = (area.right - area.left, area.bottom - area.top);
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

/// Hands the paused app's pages back to Windows (146 MB -> ~1 MB); they fault back in from standby on resume.
pub fn trim_memory() {
    unsafe { K32EmptyWorkingSet(GetCurrentProcess()) };
}

/// EcoQoS (Task Manager's efficiency mode, without its idle priority): Windows runs the app on the
/// E-cores at a low clock, leaving the P-cores to the user. A frame's ~2 ms of CPU has room to spare.
pub fn efficiency_mode() {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };
    let ok = unsafe { SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, (&raw const state).cast(), size_of_val(&state) as u32) };
    if ok == 0 {
        log::warn!("efficiency mode: error {}", unsafe { GetLastError() });
    }
}

// For Desktop::covered: marks the monitor a maximised window, or a fullscreen foreground one, hides.
unsafe extern "system" fn check_covering(hwnd: HWND, out: LPARAM) -> BOOL {
    unsafe {
        let (monitors, covered) = &mut *(out as *mut (Vec<Monitor>, Vec<bool>));
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || cloaked(hwnd) || is_shell(hwnd) {
            return 1;
        }
        // Only the monitor the window is mostly on; one we don't cover doesn't count.
        let on = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        let Some(i) = monitors.iter().position(|m| m.handle == on) else { return 1 };
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut r);
        let m = monitors[i].rect;
        let fullscreen = hwnd == GetForegroundWindow() && r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        if IsZoomed(hwnd) != 0 || fullscreen {
            covered[i] = true;
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
