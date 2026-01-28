use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub user_location: String,
    pub time: String,
    pub units: String,
}

fn get_config_path() -> PathBuf {
    let mut config_path = dirs::home_dir().expect("Could not find home directory");
    config_path.push(".terminal-weather");
    config_path.push("config.toml");
    config_path
}

pub fn load_or_create_config() -> Config {
    let config_path = get_config_path();
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).expect("Could not create config directory");
        }
        let default = Config {
            user_location: "London,UK".to_string(),
            time: "12:00".to_string(),
            units: "metric".to_string(),
        };
        let toml = toml::to_string_pretty(&default).expect("Could not serialize default config");
        fs::write(&config_path, toml).expect("Could not write config file");
        default
    } else {
        let content = fs::read_to_string(&config_path).expect("Could not read config file");
        toml::from_str(&content).expect("Could not parse config file")
    }
}