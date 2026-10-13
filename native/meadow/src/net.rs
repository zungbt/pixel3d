//! HTTPS over WinHTTP (no extra crates), and the chosen city's real weather, fetched every half
//! hour on a short-lived thread. Offline or stale, the clouds go back to their made-up weather.
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use meadow_core::weather::{self, Weather};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::*;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

const EVERY: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
const STALE: Duration = Duration::from_secs(2 * 60 * 60);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// Closes a WinHTTP handle when dropped.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn open(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() { Err(format!("{what} failed ({})", unsafe { GetLastError() })) } else { Ok(Handle(h)) }
}

fn check(ok: i32, what: &str) -> Result<(), String> {
    if ok == 0 { Err(format!("{what} failed ({})", unsafe { GetLastError() })) } else { Ok(()) }
}

/// GET https://`host``path`: the body, if the reply is 200.
pub fn get(host: &str, path: &str) -> Result<String, String> {
    unsafe {
        let session = open(WinHttpOpen(wide("pixel3d meadow").as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0), "WinHttpOpen")?;
        check(WinHttpSetTimeouts(session.0, 5000, 5000, 10_000, 10_000), "WinHttpSetTimeouts")?;
        let connect = open(WinHttpConnect(session.0, wide(host).as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0), "WinHttpConnect")?;
        let request = open(WinHttpOpenRequest(connect.0, wide("GET").as_ptr(), wide(path).as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE), "WinHttpOpenRequest")?;
        check(WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0), "WinHttpSendRequest")?;
        check(WinHttpReceiveResponse(request.0, null_mut()), "WinHttpReceiveResponse")?;
        let (mut status, mut size) = (0u32, 4u32);
        let query = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
        check(WinHttpQueryHeaders(request.0, query, null(), (&raw mut status).cast(), &mut size, null_mut()), "WinHttpQueryHeaders")?;
        let mut body = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut read = 0u32;
            check(WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read), "WinHttpReadData")?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        let body = String::from_utf8(body).map_err(|_| "reply is not UTF-8".to_string())?;
        if status != 200 {
            return Err(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>()));
        }
        Ok(body)
    }
}

/// The chosen city's weather, kept fresh.
pub struct LiveWeather {
    place: Option<(f64, f64)>, // None: no city chosen, nothing fetched
    latest: Option<(Weather, Instant)>,
    next: Option<Instant>, // None while a fetch is under way
}

impl LiveWeather {
    pub fn new(place: Option<(f64, f64)>) -> Self {
        LiveWeather { place, latest: None, next: Some(Instant::now()) }
    }

    pub fn set_place(&mut self, place: (f64, f64)) {
        self.place = Some(place);
        self.latest = None;
        self.next = self.next.map(|_| Instant::now());
    }

    /// Starts a fetch if one is due.
    pub fn poll(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let (Some(place), Some(next)) = (self.place, self.next) else { return };
        if Instant::now() < next {
            return;
        }
        self.next = None;
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let reply = get(weather::FORECAST_HOST, &weather::current_path(place)).and_then(|body| weather::parse_current(&body));
            let _ = proxy.send_event(UserEvent::Weather(place, reply));
        });
    }

    /// A fetch for `place` came back.
    pub fn got(&mut self, place: (f64, f64), reply: Result<Weather, String>) {
        let now = Instant::now();
        if Some(place) != self.place {
            self.next = Some(now); // the city changed meanwhile: fetch the new one's
            return;
        }
        match reply {
            Ok(w) => {
                log::info!("weather: cloud {:.0}%, wind from {:.0}° at {:.1} km/h, gusts {:.1}", w.cover * 100.0, w.wind_from, w.wind_kmh, w.gust_kmh);
                self.latest = Some((w, now));
                self.next = Some(now + EVERY);
            }
            Err(e) => {
                log::warn!("weather: {e}; trying again in 5 min");
                self.next = Some(now + RETRY);
            }
        }
    }

    /// The real weather, unless there's none yet or it's hours old.
    pub fn current(&self) -> Option<Weather> {
        self.latest.filter(|(_, at)| at.elapsed() < STALE).map(|(w, _)| w)
    }
}
