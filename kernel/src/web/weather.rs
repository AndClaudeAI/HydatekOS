//! The weather on the desktop card, from Open-Meteo: free, with no account
//! or key, and open source, so it can be self-hosted (the `weatherapi=`
//! line in the settings file points both lookups at another server).
//!
//! The town you type is looked up once (geocoding) and remembered with its
//! coordinates; the current temperature and sky are then asked for every
//! half hour while the computer is online.

use super::json;
use super::WebQueue;
use alloc::format;
use alloc::string::String;

pub const FORECAST: &str = "https://api.open-meteo.com";
pub const GEOCODING: &str = "https://geocoding-api.open-meteo.com";
/// How often to ask again (ms), and how soon after a failure.
pub const EVERY: u64 = 30 * 60 * 1000;
pub const RETRY: u64 = 5 * 60 * 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// The weather now: °C, rounded, the WMO weather code, day or night.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Now {
    pub temp: i32,
    pub code: u8,
    pub day: bool,
}

/// What the sky looks like, for the card's picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sky {
    Clear,
    PartCloud,
    Cloud,
    Fog,
    Rain,
    Snow,
    Storm,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// no town chosen
    Off,
    Waiting,
    /// the town wasn't found
    Unknown,
    Failed(String),
    Ready,
}

pub fn geocode_url(base: &str, town: &str) -> String {
    format!("{}/v1/search?name={}&count=1&language=en&format=json", base.trim_end_matches('/'), super::url::encode(town.trim()))
}

pub fn forecast_url(base: &str, p: &Place) -> String {
    format!("{}/v1/forecast?latitude={:.3}&longitude={:.3}&current=temperature_2m,weather_code,is_day&timezone=auto", base.trim_end_matches('/'), p.lat, p.lon)
}

/// The first match of a geocoding answer.
pub fn parse_place(body: &str) -> Option<Place> {
    let v = json::parse(body)?;
    let r = v.get("results").arr().first()?;
    Some(Place { name: String::from(r.get("name").str()?), lat: r.get("latitude").num()?, lon: r.get("longitude").num()? })
}

pub fn parse_now(body: &str) -> Option<Now> {
    let v = json::parse(body)?;
    let c = v.get("current");
    let t = c.get("temperature_2m").num()?;
    let code = c.get("weather_code").num().unwrap_or(0.0);
    let day = c.get("is_day").num().map_or(true, |d| d != 0.0);
    Some(Now { temp: round(t), code: code.clamp(0.0, 255.0) as u8, day })
}

fn round(v: f64) -> i32 {
    if v < 0.0 {
        -((-v + 0.5) as i32)
    } else {
        (v + 0.5) as i32
    }
}

/// WMO weather interpretation codes (as Open-Meteo documents them).
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 | 77 => "Snow",
        80 | 81 => "Showers",
        82 => "Heavy showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Storm with hail",
        _ => "Unsettled",
    }
}

pub fn sky(code: u8) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartCloud,
        3 => Sky::Cloud,
        45 | 48 => Sky::Fog,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Storm,
        51..=67 | 80..=82 => Sky::Rain,
        _ => Sky::Cloud,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Find,
    Fetch,
}

#[derive(Debug)]
pub struct Weather {
    /// the town as typed in Settings
    pub town: String,
    /// another Open-Meteo server for both lookups (empty: the public one)
    pub base: String,
    pub place: Option<Place>,
    pub now: Option<Now>,
    pub status: Status,
    job: Option<(u32, Step)>,
    /// when the last answer (or failure) came, in ms
    last: Option<u64>,
}

impl Default for Weather {
    fn default() -> Weather {
        Weather { town: String::new(), base: String::new(), place: None, now: None, status: Status::Off, job: None, last: None }
    }
}

impl Weather {
    /// A new town: look it up afresh.
    pub fn set_town(&mut self, town: &str, web: &mut WebQueue) {
        if let Some((id, _)) = self.job.take() {
            web.stop(id);
        }
        self.town = String::from(town.trim());
        self.place = None;
        self.now = None;
        self.last = None;
        self.status = if self.town.is_empty() { Status::Off } else { Status::Waiting };
    }

    /// A place already known (from the settings file): no lookup needed.
    pub fn restore(&mut self, town: &str, place: Option<Place>) {
        self.town = String::from(town.trim());
        self.place = place;
        self.status = if self.town.is_empty() { Status::Off } else { Status::Waiting };
    }

    fn base(&self, default: &'static str) -> String {
        if self.base.is_empty() {
            String::from(default)
        } else {
            self.base.clone()
        }
    }

    /// Collect answers and ask again when it's time. Returns true when what
    /// the card shows changed (and, for a new place, the settings need saving).
    pub fn poll(&mut self, web: &mut WebQueue, now_ms: u64, online: bool) -> bool {
        if let Some((id, step)) = self.job {
            let Some(res) = web.take(id) else { return false };
            self.job = None;
            self.last = Some(now_ms);
            let body = match res {
                Ok(r) if r.status == 200 => String::from_utf8_lossy(&r.body).into_owned(),
                Ok(r) => {
                    self.status = Status::Failed(format!("The weather service answered {}", r.status));
                    return true;
                }
                Err(e) => {
                    self.status = Status::Failed(e);
                    return true;
                }
            };
            match step {
                Step::Find => match parse_place(&body) {
                    Some(p) => {
                        self.place = Some(p);
                        self.last = None; // fetch straight away
                    }
                    None => self.status = Status::Unknown,
                },
                Step::Fetch => match parse_now(&body) {
                    Some(n) => {
                        self.now = Some(n);
                        self.status = Status::Ready;
                    }
                    None => self.status = Status::Failed(String::from("The weather service's answer wasn't understood")),
                },
            }
            return true;
        }
        if self.town.is_empty() || !online || self.status == Status::Unknown {
            return false;
        }
        let wait = match self.status {
            Status::Failed(_) => RETRY,
            _ => EVERY,
        };
        if self.last.map_or(false, |t| now_ms < t + wait) {
            return false;
        }
        match &self.place {
            None => {
                let url = geocode_url(&self.base(GEOCODING), &self.town);
                self.job = Some((web.get_api(&url, alloc::vec![]), Step::Find));
            }
            Some(p) => {
                let url = forecast_url(&self.base(FORECAST), p);
                self.job = Some((web.get_api(&url, alloc::vec![]), Step::Fetch));
            }
        }
        false
    }

    /// The settings file's lines for this.
    pub fn save(&self) -> String {
        let mut s = format!("weather={}\n", self.town);
        if let Some(p) = &self.place {
            s.push_str(&format!("weatherat={:.4},{:.4},{}\n", p.lat, p.lon, p.name));
        }
        if !self.base.is_empty() {
            s.push_str(&format!("weatherapi={}\n", self.base));
        }
        s
    }
}

/// A `weatherat=` line's value: lat,lon,name.
pub fn parse_at(v: &str) -> Option<Place> {
    let mut it = v.splitn(3, ',');
    let lat = parse_f64(it.next()?)?;
    let lon = parse_f64(it.next()?)?;
    Some(Place { name: String::from(it.next().unwrap_or("").trim()), lat, lon })
}

fn parse_f64(s: &str) -> Option<f64> {
    json::parse(s.trim())?.num()
}
