//! The tray's city search: a small window popping up by the tray (where the menu was clicked), on
//! its own thread (with its own message loop, so Tab, Enter and Esc work), that looks names up on
//! Open-Meteo and sends the chosen city back. Like the tray's flyouts, clicking away closes it.
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

use meadow_core::weather::{self, City};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontW, DeleteObject, GetMonitorInfoW, GetSysColorBrush, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

const CHOOSE: i32 = 3;
const LIST: i32 = 4;
const EDIT: i32 = 5;
const EM_SETSEL: u32 = 0x00b1; // in Win32_UI_Controls, a big feature for one constant

static OPEN: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut()); // the window, while it's up

struct State {
    proxy: EventLoopProxy<UserEvent>,
    edit: HWND,
    list: HWND,
    found: Vec<City>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Opens the search (or brings it to the front), the box filled with `current`.
pub fn open(proxy: EventLoopProxy<UserEvent>, current: Option<String>) {
    let up = OPEN.load(Ordering::SeqCst);
    if !up.is_null() {
        unsafe { SetForegroundWindow(up) };
        return;
    }
    let mut at = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut at) }; // still by the tray icon
    std::thread::spawn(move || unsafe { run(proxy, current.unwrap_or_default(), at) });
}

unsafe fn run(proxy: EventLoopProxy<UserEvent>, current: String, at: POINT) {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("pixel3d-city");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc); // fails harmlessly the second time
        let dpi = GetDpiForSystem() as i32;
        let px = |v: i32| v * dpi / 96;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut r = RECT { left: 0, top: 0, right: px(360), bottom: px(290) };
        AdjustWindowRectEx(&mut r, style, 0, WS_EX_DLGMODALFRAME | WS_EX_TOOLWINDOW);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        // Its corner at the click, kept inside the work area (off the taskbar, wherever that is).
        let mut monitor = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        GetMonitorInfoW(MonitorFromPoint(at, MONITOR_DEFAULTTOPRIMARY), &mut monitor);
        let (work, gap) = (monitor.rcWork, px(8));
        let x = (at.x - w).clamp(work.left + gap, (work.right - w - gap).max(work.left + gap));
        let y = (at.y - h).clamp(work.top + gap, (work.bottom - h - gap).max(work.top + gap));
        let ex = WS_EX_DLGMODALFRAME | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        let hwnd = CreateWindowExW(ex, class.as_ptr(), wide("Chọn thành phố").as_ptr(), style, x, y, w, h, null_mut(), null_mut(), instance, null());
        if hwnd.is_null() {
            return log::warn!("city search: no window");
        }
        OPEN.store(hwnd, Ordering::SeqCst);
        let font = CreateFontW(-(9 * dpi / 72), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, wide("Segoe UI").as_ptr());
        let child = |ex, class: &str, text: &str, style, (x, y, w, h), id: i32| {
            let c = CreateWindowExW(ex, wide(class).as_ptr(), wide(text).as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | style, px(x), px(y), px(w), px(h), hwnd, id as usize as HMENU, instance, null());
            SendMessageW(c, WM_SETFONT, font as WPARAM, 0);
            c
        };
        let edit = child(WS_EX_CLIENTEDGE, "EDIT", &current, ES_AUTOHSCROLL as u32, (12, 12, 256, 26), EDIT);
        child(0, "BUTTON", "Tìm", BS_DEFPUSHBUTTON as u32, (276, 12, 72, 26), IDOK);
        let list = child(WS_EX_CLIENTEDGE, "LISTBOX", "", WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32, (12, 48, 336, 192), LIST);
        child(0, "BUTTON", "Chọn", BS_PUSHBUTTON as u32, (196, 250, 72, 28), CHOOSE);
        child(0, "BUTTON", "Hủy", BS_PUSHBUTTON as u32, (276, 250, 72, 28), IDCANCEL);
        STATE.set(Some(State { proxy, edit, list, found: vec![] }));
        SendMessageW(edit, EM_SETSEL, 0, -1);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(edit);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        STATE.set(None);
        OPEN.store(null_mut(), Ordering::SeqCst);
        DeleteObject(font);
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam & 0xffff) as i32, ((wparam >> 16) & 0xffff) as u32);
                let Some((edit, list)) = STATE.with_borrow(|s| s.as_ref().map(|s| (s.edit, s.list))) else { return 0 };
                match id {
                    // Enter: in the list it chooses, elsewhere it searches.
                    IDOK if GetFocus() == list => choose(hwnd, list),
                    IDOK => search(edit, list),
                    CHOOSE => choose(hwnd, list),
                    LIST if code == LBN_DBLCLK => choose(hwnd, list),
                    IDCANCEL => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            // Clicked away: closed, after whatever is under way (choosing destroys it already).
            WM_ACTIVATE if wparam & 0xffff == WA_INACTIVE as usize => {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe fn search(edit: HWND, list: HWND) {
    unsafe {
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        let name = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
        if name.trim().is_empty() {
            return;
        }
        let cursor = SetCursor(LoadCursorW(null_mut(), IDC_WAIT));
        let reply = crate::net::get(weather::SEARCH_HOST, &weather::search_path(&name)).and_then(|body| weather::parse_search(&body));
        SetCursor(cursor);
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        let add = |text: &str| SendMessageW(list, LB_ADDSTRING, 0, wide(text).as_ptr() as LPARAM);
        let found = match reply {
            Ok(found) if found.is_empty() => {
                add("Không tìm thấy");
                found
            }
            Ok(found) => {
                for c in &found {
                    add(&if c.detail.is_empty() { c.name.clone() } else { format!("{}, {}", c.name, c.detail) });
                }
                found
            }
            Err(e) => {
                log::warn!("city search: {e}");
                add("Không kết nối được");
                vec![]
            }
        };
        let any = !found.is_empty();
        STATE.with_borrow_mut(|s| s.as_mut().map(|s| s.found = found));
        if any {
            SendMessageW(list, LB_SETCURSEL, 0, 0);
            SetFocus(list);
        }
    }
}

unsafe fn choose(hwnd: HWND, list: HWND) {
    unsafe {
        let sel = SendMessageW(list, LB_GETCURSEL, 0, 0);
        let chosen = STATE.with_borrow(|s| s.as_ref().and_then(|s| Some((s.proxy.clone(), usize::try_from(sel).ok().and_then(|i| s.found.get(i))?.clone()))));
        if let Some((proxy, city)) = chosen {
            let _ = proxy.send_event(UserEvent::City(city));
            DestroyWindow(hwnd);
        }
    }
}
