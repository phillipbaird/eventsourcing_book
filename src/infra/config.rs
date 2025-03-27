use anyhow::Context;
use camino::Utf8PathBuf;
use config::Config;
use serde::Deserialize;
use serde_aux::field_attributes::deserialize_number_from_string;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Debug)]
pub struct Settings {
    pub environment: String,
    pub application: ServerSettings,
    pub database: DatabaseSettings,
    pub kafka: KafkaSettings,
}

#[derive(Clone, Deserialize, Debug)]
pub struct ServerSettings {
    pub host: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub logs_directory: String,
}

impl ServerSettings {
    pub fn address(&self) -> String {
        format!("{}:{}", &self.host, &self.port)
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub database_name: String,
    pub require_ssl: bool,
}

impl DatabaseSettings {
    pub fn without_db_name(&self) -> PgConnectOptions {
        let ssl_mode = if self.require_ssl {
            PgSslMode::Require
        } else {
            PgSslMode::Prefer
        };

        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(&self.password)
            .port(self.port)
            .ssl_mode(ssl_mode)
    }

    pub fn with_db_name(&self) -> PgConnectOptions {
        self.without_db_name().database(&self.database_name)
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct KafkaSettings {
    pub bootstrap_servers: String,
    pub group_id: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub session_timeout_ms: u16,
}

fn find_config_dir() -> anyhow::Result<PathBuf> {
    let current_dir =
        std::env::current_dir().context("Failed to determine the current directory.")?;
    let current_dir =
        Utf8PathBuf::try_from(current_dir).context("Could not convert PathBuf to Utf8PathBuf")?;

    current_dir
        .ancestors()
        .map(|p| p.join("config"))
        .find(|p| {
            let base_path = p.join("base.yaml");
            p.exists() && p.is_dir() && base_path.exists() && base_path.is_file()
        })
        .map(|p| p.canonicalize().unwrap())
        .ok_or_else(|| anyhow::anyhow!("Cannot find config directory!"))
}

pub fn get_config_settings() -> anyhow::Result<Settings> {
    let config_directory = find_config_dir()?;

    // Detect the running environment - default to `development` if unspecified.
    let environment: String =
        std::env::var("APP_ENVIRONMENT").unwrap_or_else(|_| "development".to_owned());

    // Read a the base configuration file called "base".
    let base_source = config::File::from(config_directory.join("base")).required(true);

    // Read another file for environment-specific values.
    let env_source = config::File::from(config_directory.join(environment.as_str())).required(true);

    // Finally grab any override settings from environment variables
    // (with a prefix of APP and '__' as separator).
    // e.g. `APP_APPLICATION__PORT=5001 would set `Settings.application.port`
    let overrides_source = config::Environment::with_prefix("app").separator("__");

    let config = Config::builder()
        .add_source(base_source)
        .add_source(env_source)
        .add_source(overrides_source)
        .build()?;

    // Try converting the configuration values into our Settings type.
    config
        .try_deserialize()
        .context("Could not deserialise config settings.")
}
