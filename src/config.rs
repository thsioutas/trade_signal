use anyhow::{Context, Result};

impl Config {
    pub fn load(path: PathBuf) -> Result<Self, ConfigError> {
        let filename = path
            .into_os_string()
            .into_string()
            .map_err(|_| ConfigError::FilePathIntoString)?;
        let config = config::Config::builder()
            .add_source(config::File::with_name(&filename))
            .build()
            .map_err(|err| ConfigError::SettingsInit(err.to_string()))?
            .try_deserialize()
            .map_err(|err| ConfigError::Deserialize(err.to_string()))?;
        Ok(config)
    }
}
