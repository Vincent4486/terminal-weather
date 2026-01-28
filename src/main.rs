pub mod config;
use config::load_or_create_config;

fn main() {
    let config = load_or_create_config();
    println!("Loaded config: {:?}", config);
}
