pub mod routes;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::info;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalRef {
    Name(String),
    Id(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalConfig {
    #[serde(flatten)]
    pub signal: SignalRef,
    pub frequency_hz: f32,
    pub min_frequency_hz: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub signals: Vec<SignalConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub description: Option<String>,
    pub buckets: BTreeMap<String, Bucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub profiles: BTreeMap<String, Profile>,
}

// --- State ---

#[derive(Debug)]
struct State {
    config_dir: String,
    tx: watch::Sender<Option<ProfileConfig>>,
    rx: watch::Receiver<Option<ProfileConfig>>,
}

static STATE: OnceLock<State> = OnceLock::new();

fn state() -> &'static State {
    STATE.get().expect("config::init must be called before use")
}

// --- Initialization ---

pub fn init(config_dir: String) {
    let (tx, rx) = watch::channel(None);
    STATE
        .set(State { config_dir, tx, rx })
        .expect("config::init called twice");
}

// --- Public API ---

pub fn subscribe() -> watch::Receiver<Option<ProfileConfig>> {
    state().tx.subscribe()
}

pub fn current() -> Option<ProfileConfig> {
    state().rx.borrow().clone()
}

pub fn get_text() -> String {
    match current() {
        Some(config) => toml::to_string_pretty(&config).expect("Config should always serialize"),
        None => toml::to_string_pretty(&example()).expect("Example should always serialize"),
    }
}

pub async fn apply_and_save(input: &str) -> Result<()> {
    let config = parse(input)?;
    write_to_disk(input).await?;
    state().tx.send(Some(config)).ok();
    Ok(())
}

pub async fn load_from_disk() -> Result<()> {
    let path = config_path();
    if path.exists() {
        let text = tokio::fs::read_to_string(&path)
            .await
            .context("Failed to read config from disk")?;
        let config = parse(&text)?;
        state().tx.send(Some(config)).ok();
    }
    Ok(())
}

// --- Internal ---

fn config_path() -> PathBuf {
    PathBuf::from(&state().config_dir).join("config.toml")
}

async fn write_to_disk(input: &str) -> Result<()> {
    let path = config_path();
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .context("Failed to create config directory")?;
    tokio::fs::write(&path, input)
        .await
        .context("Failed to write config to disk")?;

    let absolute_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    info!("New config saved to {}", absolute_path.display());
    Ok(())
}

fn parse(input: &str) -> Result<ProfileConfig> {
    // FIXME: this from_str will just drop any parts it can't parse instead of erroring
    let config: ProfileConfig = toml::from_str(input).context("Failed to parse config TOML")?;
    validate(&config)?;
    Ok(config)
}

fn validate(config: &ProfileConfig) -> Result<()> {
    for (profile_name, profile) in &config.profiles {
        if profile.buckets.is_empty() {
            bail!("Profile '{}' has no buckets", profile_name);
        }
        for (bucket_key, bucket) in &profile.buckets {
            if bucket_key.parse::<u32>().is_err() {
                bail!(
                    "Profile '{}': bucket key '{}' is not a valid integer",
                    profile_name,
                    bucket_key
                );
            }
            if bucket.signals.is_empty() {
                bail!(
                    "Profile '{}', bucket '{}' has no signals",
                    profile_name,
                    bucket_key
                );
            }
            for signal in &bucket.signals {
                if signal.frequency_hz <= 0.0 {
                    bail!(
                        "Profile '{}', bucket '{}': frequency_hz must be greater than 0",
                        profile_name,
                        bucket_key
                    );
                }
                if signal.min_frequency_hz < 0.0 {
                    bail!(
                        "Profile '{}', bucket '{}': min_frequency_hz must be >= 0",
                        profile_name,
                        bucket_key
                    );
                }
                if signal.min_frequency_hz > signal.frequency_hz {
                    bail!(
                        "Profile '{}', bucket '{}': min_frequency_hz cannot exceed frequency_hz",
                        profile_name,
                        bucket_key
                    );
                }
            }
        }
    }
    Ok(())
}

fn example() -> ProfileConfig {
    ProfileConfig {
        profiles: BTreeMap::from([(
            "acceleration".to_string(),
            Profile {
                description: Some("FSAE Acceleration Event".to_string()),
                buckets: BTreeMap::from([
                    (
                        "0".to_string(),
                        Bucket {
                            signals: vec![
                                SignalConfig {
                                    signal: SignalRef::Name("AccuCellHighTemp".to_string()),
                                    frequency_hz: 1.0,
                                    min_frequency_hz: 1.0,
                                },
                                SignalConfig {
                                    signal: SignalRef::Name("velX".to_string()),
                                    frequency_hz: 10.0,
                                    min_frequency_hz: 1.0,
                                },
                            ],
                        },
                    ),
                    (
                        "1".to_string(),
                        Bucket {
                            signals: vec![
                                SignalConfig {
                                    signal: SignalRef::Name("MotorSpeed".to_string()),
                                    frequency_hz: 10.0,
                                    min_frequency_hz: 10.0,
                                },
                                SignalConfig {
                                    signal: SignalRef::Name("LWS_ANGLE".to_string()),
                                    frequency_hz: 10.0,
                                    min_frequency_hz: 5.0,
                                },
                            ],
                        },
                    ),
                    (
                        "2".to_string(),
                        Bucket {
                            signals: vec![SignalConfig {
                                signal: SignalRef::Name("RearBrakePressure".to_string()),
                                frequency_hz: 50.0,
                                min_frequency_hz: 0.0,
                            }],
                        },
                    ),
                ]),
            },
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_by_name() {
        let toml = r#"
            [profiles.acceleration.buckets.0]
            signals = [
                { name = "MotorSpeed", frequency_hz = 100.0, min_frequency_hz = 10.0 },
            ]
        "#;
        let config = parse(toml).unwrap();
        let signal = &config.profiles["acceleration"].buckets[&0].signals[0];
        assert!(matches!(signal.signal, SignalRef::Name(_)));
    }

    #[test]
    fn test_signal_by_id() {
        let toml = r#"
            [profiles.acceleration.buckets.0]
            signals = [
                { id = 291, frequency_hz = 100.0, min_frequency_hz = 10.0 },
            ]
        "#;
        let config = parse(toml).unwrap();
        let signal = &config.profiles["acceleration"].buckets[&0].signals[0];
        assert!(matches!(signal.signal, SignalRef::Id(291)));
    }

    #[test]
    fn test_bucket_ordering() {
        let toml = r#"
            [profiles.acceleration.buckets.10]
            signals = [{ name = "velX", frequency_hz = 50.0, min_frequency_hz = 5.0 }]

            [profiles.acceleration.buckets.1]
            signals = [{ name = "MotorSpeed", frequency_hz = 100.0, min_frequency_hz = 10.0 }]
        "#;
        let config = parse(toml).unwrap();
        let mut keys: Vec<u32> = config.profiles["acceleration"]
            .buckets
            .keys()
            .map(|k| k.parse().unwrap())
            .collect();
        keys.sort();
        assert_eq!(keys, vec![1, 10]);
    }

    #[test]
    fn test_invalid_bucket_key() {
        let toml = r#"
            [profiles.acceleration.buckets.abc]
            signals = [{ name = "MotorSpeed", frequency_hz = 100.0, min_frequency_hz = 10.0 }]
        "#;
        assert!(parse(toml).is_err());
    }

    #[test]
    fn test_min_exceeds_max() {
        let toml = r#"
            [profiles.acceleration.buckets.0]
            signals = [{ name = "MotorSpeed", frequency_hz = 10.0, min_frequency_hz = 100.0 }]
        "#;
        assert!(parse(toml).is_err());
    }

    #[test]
    fn test_invalid_frequency() {
        let toml = r#"
            [profiles.acceleration.buckets.0]
            signals = [{ name = "MotorSpeed", frequency_hz = -1.0, min_frequency_hz = 0.0 }]
        "#;
        assert!(parse(toml).is_err());
    }

    #[test]
    fn test_example_serializes() {
        let text = toml::to_string_pretty(&example()).unwrap();
        assert!(parse(&text).is_ok());
    }
}
