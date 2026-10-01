use crate::web::weather::*;
use crate::web::{Response, WebQueue};

// shaped like Open-Meteo's documented answers
const GEO: &str = r#"{"results":[{"id":2325590,"name":"Owerri","latitude":5.48333,"longitude":7.03041,"elevation":66.0,"country_code":"NG","timezone":"Africa/Lagos","country":"Nigeria"}],"generationtime_ms":0.6}"#;
const NOW: &str = r#"{"latitude":5.5,"longitude":7.0,"timezone":"Africa/Lagos","current_units":{"temperature_2m":"°C"},"current":{"time":"2026-09-27T10:30","interval":900,"temperature_2m":27.6,"weather_code":2,"is_day":1}}"#;

fn ok(body: &str) -> Result<Response, String> {
    Ok(Response { url: String::new(), status: 200, headers: vec![], body: body.as_bytes().to_vec(), security: None })
}

#[test]
fn urls() {
    assert_eq!(geocode_url(GEOCODING, "Port Harcourt"), "https://geocoding-api.open-meteo.com/v1/search?name=Port+Harcourt&count=1&language=en&format=json");
    let p = Place { name: "Owerri".into(), lat: 5.48333, lon: 7.03041 };
    assert_eq!(forecast_url("http://10.0.2.2:8080/", &p), "http://10.0.2.2:8080/v1/forecast?latitude=5.483&longitude=7.030&current=temperature_2m,weather_code,is_day&timezone=auto");
}

#[test]
fn parses() {
    assert_eq!(parse_place(GEO), Some(Place { name: "Owerri".into(), lat: 5.48333, lon: 7.03041 }));
    assert_eq!(parse_place(r#"{"generationtime_ms":0.2}"#), None);
    assert_eq!(parse_now(NOW), Some(Now { temp: 28, code: 2, day: true }));
    let cold = NOW.replace("27.6", "-3.5").replace("\"is_day\":1", "\"is_day\":0");
    assert_eq!(parse_now(&cold), Some(Now { temp: -4, code: 2, day: false }));
    assert_eq!(describe(2), "Partly cloudy");
    assert_eq!(sky(63), Sky::Rain);
    assert_eq!(sky(95), Sky::Storm);
    assert_eq!(sky(73), Sky::Snow);
}

#[test]
fn saved_lines_round_trip() {
    let mut w = Weather::default();
    w.restore("Owerri", parse_at("5.4833,7.0304,Owerri"));
    assert!(w.save().contains("weather=Owerri\nweatherat=5.4833,7.0304,Owerri\n"));
    assert_eq!(parse_at("x,1"), None);
}

#[test]
fn looks_up_then_fetches_then_waits() {
    let mut web = WebQueue::default();
    let mut w = Weather::default();
    w.set_town("Owerri", &mut web);
    // offline: nothing asked
    assert!(!w.poll(&mut web, 0, false));
    assert!(web.queue.is_empty());
    assert!(!w.poll(&mut web, 0, true));
    let r = web.queue.pop().unwrap();
    assert!(r.url.starts_with("https://geocoding-api.open-meteo.com/v1/search?name=Owerri"));
    web.done.push((r.id, ok(GEO)));
    assert!(w.poll(&mut web, 10, true));
    assert_eq!(w.place.as_ref().unwrap().name, "Owerri");
    // the forecast at once
    w.poll(&mut web, 20, true);
    let r = web.queue.pop().unwrap();
    assert!(r.url.contains("/v1/forecast?latitude=5.483&longitude=7.030"));
    web.done.push((r.id, ok(NOW)));
    assert!(w.poll(&mut web, 30, true));
    assert_eq!(w.status, Status::Ready);
    assert_eq!(w.now.unwrap().temp, 28);
    // and not again for half an hour
    w.poll(&mut web, 30 + EVERY - 1, true);
    assert!(web.queue.is_empty());
    w.poll(&mut web, 30 + EVERY, true);
    assert_eq!(web.queue.len(), 1);
}

#[test]
fn unknown_town_and_failures() {
    let mut web = WebQueue::default();
    let mut w = Weather::default();
    w.set_town("Nowhereville", &mut web);
    w.poll(&mut web, 0, true);
    let r = web.queue.pop().unwrap();
    web.done.push((r.id, ok(r#"{"generationtime_ms":0.2}"#)));
    w.poll(&mut web, 1, true);
    assert_eq!(w.status, Status::Unknown);
    w.poll(&mut web, 1_000_000_000, true);
    assert!(web.queue.is_empty(), "an unknown town isn't asked for again");

    w.set_town("Owerri", &mut web);
    w.poll(&mut web, 0, true);
    let r = web.queue.pop().unwrap();
    web.done.push((r.id, Err("No network".into())));
    w.poll(&mut web, 100, true);
    assert!(matches!(w.status, Status::Failed(_)));
    w.poll(&mut web, 100 + RETRY - 1, true);
    assert!(web.queue.is_empty());
    w.poll(&mut web, 100 + RETRY, true);
    assert_eq!(web.queue.len(), 1);
}
