# Terminal Weather

A simple CLI tool to check the weather using the OpenWeatherMap API.

## Setup

1.  Get an API key from [OpenWeatherMap](https://openweathermap.org/api).
2.  Create a `.env` file in the root directory (you can copy `.env.example`):
    ```bash
    cp .env.example .env
    ```
3.  Add your API key to `.env`:
    ```
    OPENWEATHER_API_KEY=your_key_here
    WEATHER_CITY=London
    ```

## Usage

Run with cargo:

```bash
cargo run
```
