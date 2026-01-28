use dotenvy::dotenv;
use serde::Deserialize;
use std::env;
use std::io::{self, Write};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use std::thread;
use chrono::{DateTime, Local, Utc};

const API_2_5_URL: &str = "https://api.openweathermap.org/data/2.5/weather";
const API_3_0_URL: &str = "https://api.openweathermap.org/data/3.0/onecall";
const GEO_URL: &str = "http://api.openweathermap.org/geo/1.0/direct";

// --- Structs for 2.5 API ---
#[derive(Debug, Deserialize)]
struct WeatherResponse25 {
    weather: Vec<WeatherDescription>,
    main: MainWxData25,
    wind: WindData,
    name: String,
    sys: Option<SysData25>,
    visibility: Option<u16>,
    dt: i64,
}

#[derive(Debug, Deserialize)]
struct MainWxData25 {
    temp: f64,
    humidity: u8,
    feels_like: f64,
    temp_min: f64,
    temp_max: f64,
    pressure: u16,
}

#[derive(Debug, Deserialize)]
struct SysData25 {
    sunrise: i64,
    sunset: i64,
}

// --- Structs for 3.0 API ---
#[derive(Debug, Deserialize)]
struct WeatherResponse30 {
    current: CurrentWeather30,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather30 {
    temp: f64,
    humidity: u8,
    wind_speed: f64,
    weather: Vec<WeatherDescription>,
    feels_like: f64,
    pressure: u16,
    uvi: Option<f64>,
    visibility: Option<u16>,
    sunrise: Option<i64>,
    sunset: Option<i64>,
    dt: i64,
}

// --- Unified Data Struct ---
struct WeatherDisplayData {
    name: String,
    description: String,
    temp: f64,
    humidity: u8,
    wind_speed: f64,
    // Detailed info
    feels_like: f64,
    pressure: u16,
    visibility: Option<u16>,
    sunrise: Option<i64>,
    sunset: Option<i64>,
    uvi: Option<f64>,
    temp_min: Option<f64>,
    temp_max: Option<f64>,
}

// --- Shared Structs ---
#[derive(Debug, Deserialize)]
struct WeatherDescription {
    description: String,
}

#[derive(Debug, Deserialize)]
struct WindData {
    speed: f64,
}

#[derive(Debug, Deserialize)]
struct GeoResponse {
    latitude: f64,
    longitude: f64,
    city: String,
    country_name: String,
}

#[derive(Debug, Deserialize)]
struct OwGeo {
    lat: f64,
    lon: f64,
    name: String,
    country: String,
}

struct Config {
    api_key: String,
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    detailed: bool,
}

impl Config {
    fn load() -> Result<Self, String> {
        let api_key = match env::var("OPENWEATHER_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                print!("OpenWeatherMap API Key not found in env.\nPlease enter your API key: ");
                io::stdout().flush().map_err(|e| e.to_string())?;
                
                let mut input = String::new();
                io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
                
                let key = input.trim().to_string();
                if key.is_empty() {
                    return Err("API key is required".to_string());
                }
                key
            }
        };

        // Parse CLI args
        let args: Vec<String> = env::args().skip(1).collect();
        let mut detailed = false;
        let mut manual_location = None;
        
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-m" => {
                    detailed = true;
                    i += 1;
                },
                "-l" => {
                    i += 1; // Consume flag
                    let mut parts = Vec::new();
                    // Consume args until value is a known flag or end
                    while i < args.len() && !["-m", "-l"].contains(&args[i].as_str()) {
                        parts.push(args[i].clone());
                        i += 1;
                    }
                    if !parts.is_empty() {
                        manual_location = Some(parts.join(" "));
                    }
                },
                _ => i += 1,
            }
        }

        let mut city = env::var("WEATHER_CITY").ok().filter(|s| !s.is_empty());
        let mut lat = env::var("WEATHER_LAT").ok().and_then(|s| s.parse().ok());
        let mut lon = env::var("WEATHER_LON").ok().and_then(|s| s.parse().ok());
        
        // CLI overrides Env
        if let Some(loc) = manual_location {
            // Check if coordinates (lat,lon)
            let coords: Vec<&str> = loc.split(',').collect();
            let is_coords = coords.len() == 2 
                && coords[0].trim().parse::<f64>().is_ok() 
                && coords[1].trim().parse::<f64>().is_ok();
                
            if is_coords {
                lat = coords[0].trim().parse().ok();
                lon = coords[1].trim().parse().ok();
                city = None;
            } else {
                city = Some(loc);
                lat = None;
                lon = None;
            }
        }

        Ok(Config { api_key, city, lat, lon, detailed })
    }
}

struct Location {
    lat: f64,
    lon: f64,
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let config = Config::load().expect("Failed to load configuration");
    let client = reqwest::Client::new();

    // Setup spinner for location
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
        .template("{spinner:.blue} {msg}")
        .unwrap());

    // 1. Resolve Location (Coordinates)
    // Priority: Config Manual Coords -> Config City -> IP Geolocation
    let location = if let (Some(lat), Some(lon)) = (config.lat, config.lon) {
        pb.set_message("Using manual coordinates...");
        pb.tick();
        Location { lat, lon, name: "Custom Coordinates".to_string() }
    } else if let Some(city) = &config.city {
            pb.set_message(format!("Resolving location for '{}'...", city));
            pb.enable_steady_tick(Duration::from_millis(100));

            let geo_url = format!("{}?q={}&limit=1&appid={}", GEO_URL, city, config.api_key);
            let response = client.get(&geo_url).send().await?;
            
            if !response.status().is_success() {
                 pb.finish_and_clear();
                 eprintln!("{} {}", "✖".red(), format!("Error fetching location: {}", response.status()).bright_red());
                 return Ok(());
            }

            let locations: Vec<OwGeo> = response.json().await?;
            if let Some(loc) = locations.first() {
                Location {
                    lat: loc.lat,
                    lon: loc.lon,
                    name: format!("{}, {}", loc.name, loc.country),
                }
            } else {
                pb.finish_and_clear();
                eprintln!("{} City not found.", "✖".red());
                return Ok(());
            }
    } else {
        pb.set_message("Detecting location via IP...");
        pb.enable_steady_tick(Duration::from_millis(100));
        
        let geo: GeoResponse = client.get("https://ipapi.co/json/")
            .header(reqwest::header::USER_AGENT, "terminal-weather")
            .send().await?
            .json().await?;
        
        Location {
            lat: geo.latitude,
            lon: geo.longitude,
            name: format!("{}, {}", geo.city, geo.country_name),
        }
    };

    pb.finish_with_message(format!("{} Target: {} ({:.4}, {:.4})", "✓".green(), location.name.cyan(), location.lat, location.lon));

    // 2. Fetch Weather (Priority: 2.5 -> Fallback: 3.0)
    let pb_wx = ProgressBar::new_spinner();
    pb_wx.set_style(ProgressStyle::default_spinner()
        .tick_chars("🌑Dl🌒Dl🌓Dl🌔Dl🌕Dl🌖Dl🌗Dl🌘Dl ")
        .template("{spinner:.yellow} {msg}")
        .unwrap());
    pb_wx.set_message("Fetching weather data...");
    pb_wx.enable_steady_tick(Duration::from_millis(80));

    // Try API 2.5 (Current Weather)
    let url_25 = format!("{}?lat={}&lon={}&appid={}&units=metric", API_2_5_URL, location.lat, location.lon, config.api_key);
    let res_25 = client.get(&url_25).send().await?;
    
    let weather_display: Option<WeatherDisplayData>;

    if res_25.status().is_success() {
        let weather: WeatherResponse25 = res_25.json().await?;
        weather_display = Some(WeatherDisplayData {
            name: weather.name,
            description: weather.weather.first().map(|w| w.description.clone()).unwrap_or_default(),
            temp: weather.main.temp,
            humidity: weather.main.humidity,
            wind_speed: weather.wind.speed,
            feels_like: weather.main.feels_like,
            pressure: weather.main.pressure,
            visibility: weather.visibility,
            sunrise: weather.sys.as_ref().map(|s| s.sunrise),
            sunset: weather.sys.as_ref().map(|s| s.sunset),
            uvi: None,
            temp_min: Some(weather.main.temp_min),
            temp_max: Some(weather.main.temp_max),
        });
    } else if res_25.status() == reqwest::StatusCode::UNAUTHORIZED {
         pb_wx.set_message("API 2.5 Unauthorized. Trying API 3.0...");
         
         let url_30 = format!("{}?lat={}&lon={}&appid={}&units=metric&exclude=minutely,hourly,daily,alerts", API_3_0_URL, location.lat, location.lon, config.api_key);
         let res_30 = client.get(&url_30).send().await?;
         
         if res_30.status().is_success() {
             let weather: WeatherResponse30 = res_30.json().await?;
             weather_display = Some(WeatherDisplayData {
                name: location.name.clone(),
                description: weather.current.weather.first().map(|w| w.description.clone()).unwrap_or_default(),
                temp: weather.current.temp,
                humidity: weather.current.humidity,
                wind_speed: weather.current.wind_speed,
                feels_like: weather.current.feels_like,
                pressure: weather.current.pressure,
                visibility: weather.current.visibility,
                sunrise: weather.current.sunrise,
                sunset: weather.current.sunset,
                uvi: weather.current.uvi,
                temp_min: None,
                temp_max: None,
            });
         } else {
             pb_wx.finish_and_clear();
             print_error(res_30.status(), &res_30.text().await.unwrap_or_default());
             return Ok(());
         }

    } else {
        pb_wx.finish_and_clear();
        print_error(res_25.status(), &res_25.text().await.unwrap_or_default());
        return Ok(());
    }

    if let Some(w) = weather_display {
        pb_wx.finish_and_clear();
        print_weather(&w, config.detailed);
    }

    Ok(())
}

fn print_weather(data: &WeatherDisplayData, detailed: bool) {
    let border_len = if detailed { 46 } else { 38 };
    let border = "═".repeat(border_len);
    // Animated entry
    println!();
    println!("{}", border.bright_black());
    
    print!("  Weather in ");
    io::stdout().flush().unwrap();
    // Type out the city name 
    for c in data.name.chars() {
        print!("{}", c.to_string().bold().cyan());
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(50));
    }
    println!();
    println!("{}", border.bright_black());
    thread::sleep(Duration::from_millis(200));
    
    let icon = match data.description.to_lowercase().as_str() {
        d if d.contains("clear") => "☀️ ",
        d if d.contains("cloud") => "☁️ ",
        d if d.contains("rain") => "🌧️ ",
        d if d.contains("snow") => "❄️ ",
        d if d.contains("storm") => "⛈️ ",
        d if d.contains("mist") || d.contains("fog") => "🌫️ ",
        _ => "🌡️ ",
    };
    
    // Sequential reveal of stats
    let lines = vec![
        format!("  {} {}", icon, data.description.to_title_case()),
        format!("  🌡️  Temperature: {}°C", format!("{:.1}", data.temp).yellow()),
        format!("  💧 Humidity:    {}%", format!("{}", data.humidity).blue()),
        format!("  💨 Wind Speed:  {} m/s", format!("{:.1}", data.wind_speed).green()),
    ];

    for line in lines {
        println!("{}", line);
        thread::sleep(Duration::from_millis(150));
    }

    if detailed {
        println!("{}", "─".repeat(border_len).bright_black());
        thread::sleep(Duration::from_millis(150));
        
        // Detailed lines group
        let mut detail_lines = Vec::new();
        detail_lines.push(format!("  🧐 Feels Like:  {}°C", format!("{:.1}", data.feels_like).yellow()));
        
        if let (Some(min), Some(max)) = (data.temp_min, data.temp_max) {
             detail_lines.push(format!("  📉 Min/Max:     {:.1}°C / {:.1}°C", min, max));
        }
        
        detail_lines.push(format!("  🎈 Pressure:    {} hPa", data.pressure));
        
        if let Some(vis) = data.visibility {
            let vis_km = vis as f64 / 1000.0;
            detail_lines.push(format!("  👁️  Visibility:  {:.1} km", vis_km));
        }

        if let Some(uv) = data.uvi {
            let uv_colored = if uv < 3.0 { uv.to_string().green() } 
                            else if uv < 6.0 { uv.to_string().yellow() } 
                            else { uv.to_string().red() };
            detail_lines.push(format!("  ☀️  UV Index:    {}", uv_colored));
        }

        if let Some(rise) = data.sunrise {
            let dt = DateTime::<Utc>::from_timestamp(rise, 0).unwrap_or_default();
            let local_dt: DateTime<Local> = DateTime::from(dt);
            detail_lines.push(format!("  🌅 Sunrise:     {}", local_dt.format("%H:%M")));
        }

        if let Some(set) = data.sunset {
            let dt = DateTime::<Utc>::from_timestamp(set, 0).unwrap_or_default();
            let local_dt: DateTime<Local> = DateTime::from(dt);
            detail_lines.push(format!("  🌇 Sunset:      {}", local_dt.format("%H:%M")));
        }

        for line in detail_lines {
             println!("{}", line);
             thread::sleep(Duration::from_millis(100));
        }
    }

    println!("{}\n", border.bright_black());
}

fn print_error(status: reqwest::StatusCode, error_text: &str) {
    eprintln!("{} Error fetching weather: {}", "✖".red(), status);
    eprintln!("API Response: {}", error_text);
    if status == reqwest::StatusCode::UNAUTHORIZED {
         eprintln!("\n{}:", "Troubleshooting".yellow());
         eprintln!("1. Check if your API key is valid.");
         eprintln!("2. New keys can take 10-60 minutes to activate.");
    }
}

trait TitleCase {
    fn to_title_case(&self) -> String;
}

impl TitleCase for str {
    fn to_title_case(&self) -> String {
        let mut c = self.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        }
    }
}
