use std::env;
use std::str::FromStr;

pub fn env_as<T: FromStr>(name: &str) -> Option<T> {
    env::var(name)
        .ok()
        .and_then(|s| s.parse::<T>().ok())
}