use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const DEFAULT_PORT: u16 = 7777;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub port: u16,
    pub password: String,
}

#[derive(Deserialize)]
struct ConfigFile {
    password: String,
}

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.toml")
}

pub fn uploads_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("uploads")
}

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("open-rally.db")
}

pub fn generate_password() -> String {
    use rand::distr::{Alphanumeric, SampleString};
    Alphanumeric.sample_string(&mut rand::rng(), 12)
}

fn default_data_dir() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|d| d.join("open-rally"))
        .ok_or_else(|| "데이터 폴더 위치를 찾을 수 없습니다".to_string())
}

pub fn load() -> Result<Config, String> {
    let data_dir = match std::env::var("OPEN_RALLY_DATA_DIR") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => default_data_dir()?,
    };
    let port = match std::env::var("OPEN_RALLY_PORT") {
        Ok(v) if !v.is_empty() => v
            .parse::<u16>()
            .map_err(|_| format!("OPEN_RALLY_PORT 값이 올바르지 않습니다: {v}"))?,
        _ => DEFAULT_PORT,
    };
    std::fs::create_dir_all(uploads_dir(&data_dir))
        .map_err(|e| format!("데이터 폴더를 만들 수 없습니다 ({}): {e}", data_dir.display()))?;

    let path = config_path(&data_dir);
    let password = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("config.toml 을 읽을 수 없습니다: {e}"))?;
        toml::from_str::<ConfigFile>(&text)
            .map_err(|e| format!("config.toml 형식이 올바르지 않습니다: {e}"))?
            .password
    } else {
        let pw = generate_password();
        std::fs::write(&path, format!("password = \"{pw}\"\n"))
            .map_err(|e| format!("config.toml 을 쓸 수 없습니다: {e}"))?;
        pw
    };

    Ok(Config { data_dir, port, password })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn temp_env(vars: &[(&str, &str)], f: impl FnOnce()) {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = ["OPEN_RALLY_DATA_DIR", "OPEN_RALLY_PORT"]
            .iter()
            .map(|k| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, _) in &saved {
            unsafe { std::env::remove_var(k) };
        }
        for (k, v) in vars {
            unsafe { std::env::set_var(k, v) };
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => unsafe { std::env::set_var(&k, v) },
                None => unsafe { std::env::remove_var(&k) },
            }
        }
    }

    #[test]
    fn load_creates_config_with_random_password_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        temp_env(
            &[
                ("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap()),
                ("OPEN_RALLY_PORT", "8123"),
            ],
            || {
                let cfg = load().unwrap();
                assert_eq!(cfg.port, 8123);
                assert_eq!(cfg.password.len(), 12);
                assert!(config_path(dir.path()).exists());
                assert!(uploads_dir(dir.path()).is_dir());
                let again = load().unwrap();
                assert_eq!(again.password, cfg.password);
            },
        );
    }

    #[test]
    fn load_reads_existing_password() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "password = \"hunter2\"\n").unwrap();
        temp_env(&[("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap())], || {
            assert_eq!(load().unwrap().password, "hunter2");
            assert_eq!(load().unwrap().port, 7777);
        });
    }
}
