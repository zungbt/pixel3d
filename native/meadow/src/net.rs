//! HTTPS over WinHTTP (no extra crates), and the chosen city's real weather, fetched every half
//! hour on a short-lived thread; a trip to a new city first fetches the weather along the way, in
//! one request. Offline or stale, the clouds go back to their made-up weather.
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
const FADE: Duration = Duration::from_secs(8); // the last weather shown turns into the newly fetched one over this

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

/// The chosen city's weather, kept fresh, and the weather along the trip there.
pub struct LiveWeather {
    place: Option<(f64, f64)>, // None: no city chosen, nothing fetched
    from: Option<(f64, f64)>,  // where the trip to `place` set out from, until its route's weather is in
    latest: Option<(Weather, Instant)>,
    route: Option<(Vec<Weather>, Instant)>, // at weather::route's points, from where the trip set out to `place`
    held: Option<(Weather, Instant)>, // the weather shown when the city was chosen, kept while the new one's is fetched
    next: Option<Instant>, // None while a fetch is under way
}

impl LiveWeather {
    pub fn new(place: Option<(f64, f64)>) -> Self {
        LiveWeather { place, from: None, latest: None, route: None, held: None, next: Some(Instant::now()) }
    }

    /// A trip from `from` to `place` begins, `progress` (0-1, None if arrived) of the one before behind it.
    pub fn set_place(&mut self, from: (f64, f64), place: (f64, f64), progress: Option<f64>) {
        self.held = self.shown(progress);
        self.place = Some(place);
        self.from = Some(from);
        self.latest = None;
        self.route = None;
        self.next = self.next.map(|_| Instant::now());
    }

    // The points the next fetch asks for: the whole way there first, then the city alone.
    fn points(&self) -> Option<Vec<(f64, f64)>> {
        let place = self.place?;
        Some(self.from.map_or(vec![place], |from| weather::route(from, place)))
    }

    /// Starts a fetch if one is due.
    pub fn poll(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let (Some(points), Some(next)) = (self.points(), self.next) else { return };
        if Instant::now() < next {
            return;
        }
        self.next = None;
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let reply = get(weather::FORECAST_HOST, &weather::current_path(&points)).and_then(|body| weather::parse_current(&body));
            let _ = proxy.send_event(UserEvent::Weather(points, reply));
        });
    }

    /// A fetch for `points` came back.
    pub fn got(&mut self, points: Vec<(f64, f64)>, reply: Result<Vec<Weather>, String>) {
        let now = Instant::now();
        if Some(&points) != self.points().as_ref() {
            self.next = Some(now); // the city changed meanwhile: fetch the new one's
            return;
        }
        match reply.and_then(|ws| if ws.len() == points.len() { Ok(ws) } else { Err(format!("{} places for {} asked", ws.len(), points.len())) }) {
            Ok(ws) => {
                let w = ws[ws.len() - 1];
                if ws.len() > 1 {
                    let covers: Vec<String> = ws.iter().map(|w| format!("{:.0}", w.cover * 100.0)).collect();
                    log::info!("weather on the way, {} points: cloud % {}", ws.len(), covers.join(" "));
                    self.route = Some((ws, now));
                }
                self.from = None;
                log::info!("weather: cloud {:.0}%, wind from {:.0}° at {:.1} km/h, gusts {:.1}, visibility {:.0} m, code {}", w.cover * 100.0, w.wind_from, w.wind_kmh, w.gust_kmh, w.visibility_m, w.code);
                self.latest = Some((w, now));
                self.next = Some(now + EVERY);
            }
            Err(e) => {
                log::warn!("weather: {e}; trying again in 5 min");
                self.held = None; // not this city's: back to the made-up weather
                self.from = None; // and just the city's next time
                self.next = Some(now + RETRY);
            }
        }
    }

    /// The real weather, unless there's none yet or it's hours old: on a trip `progress` (0-1)
    /// of the way, the weather there. Right after a city change the weather shown carries on,
    /// then fades into the fetched one.
    pub fn current(&self, progress: Option<f64>) -> Option<Weather> {
        self.shown(progress).map(|(w, _)| w)
    }

    // The weather now, and when the newest report it is made of arrived.
    fn shown(&self, progress: Option<f64>) -> Option<(Weather, Instant)> {
        let fresh = |r: &&(Weather, Instant)| r.1.elapsed() < STALE;
        // A trip's route stays, however long it takes: the weather on the way is what it was.
        let on_the_way = match (&self.route, progress) {
            (Some((route, at)), Some(f)) => Some((Weather::along(route, f), *at)),
            _ => self.latest.filter(|r| fresh(&r)),
        };
        match (on_the_way.as_ref(), self.held.as_ref().filter(fresh)) {
            (Some(&(new, at)), Some(&(old, _))) => {
                let f = (at.elapsed().as_secs_f64() / FADE.as_secs_f64()).min(1.0);
                Some((Weather::mix(&old, &new, meadow_core::world::smoothstep(f, 0.0, 1.0)), at))
            }
            (Some(&latest), None) => Some(latest),
            (None, held) => held.copied(),
        }
    }
}
