use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 7777;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub port: u16,
    pub password: String,
    /// 이 일수보다 오래된 글의 첨부 파일을 자동 삭제한다. 0 이면 끔.
    pub retention_days: u32,
}

#[derive(Deserialize, Serialize)]
struct ConfigFile {
    password: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    retention_days: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn read_file(path: &Path) -> Result<ConfigFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("config.toml 을 읽을 수 없습니다: {e}"))?;
    toml::from_str::<ConfigFile>(&text)
        .map_err(|e| format!("config.toml 형식이 올바르지 않습니다: {e}"))
}

fn write_file(path: &Path, file: &ConfigFile) -> Result<(), String> {
    let text = toml::to_string(file).map_err(|e| format!("config.toml 직렬화 실패: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("config.toml 을 쓸 수 없습니다: {e}"))
}

/// 자동 정리 기간을 config.toml 에 저장한다 (비밀번호는 보존). 파일이 없으면 만든다.
pub fn set_retention_days(data_dir: &Path, days: u32) -> Result<(), String> {
    let path = config_path(data_dir);
    let mut file = if path.exists() {
        read_file(&path)?
    } else {
        ConfigFile { password: generate_password(), retention_days: 0 }
    };
    file.retention_days = days;
    write_file(&path, &file)
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
    let env_password = std::env::var("OPEN_RALLY_PASSWORD")
        .ok()
        .filter(|v| !v.is_empty());
    // 파일이 있으면 읽고, 없고 환경변수 비밀번호도 없으면 새로 만든다.
    let file = if path.exists() {
        Some(read_file(&path)?)
    } else if env_password.is_none() {
        let f = ConfigFile { password: generate_password(), retention_days: 0 };
        write_file(&path, &f)?;
        Some(f)
    } else {
        None
    };
    let password = match (env_password, &file) {
        (Some(pw), _) => pw,
        (None, Some(f)) => f.password.clone(),
        (None, None) => unreachable!("file is created when no env password"),
    };
    let retention_days = match std::env::var("OPEN_RALLY_RETENTION_DAYS") {
        Ok(v) if !v.is_empty() => v
            .parse::<u32>()
            .map_err(|_| format!("OPEN_RALLY_RETENTION_DAYS 값이 올바르지 않습니다: {v}"))?,
        _ => file.as_ref().map(|f| f.retention_days).unwrap_or(0),
    };

    Ok(Config { data_dir, port, password, retention_days })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn temp_env(vars: &[(&str, &str)], f: impl FnOnce()) {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = ["OPEN_RALLY_DATA_DIR", "OPEN_RALLY_PORT", "OPEN_RALLY_PASSWORD", "OPEN_RALLY_RETENTION_DAYS"]
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
    fn env_password_overrides_file_and_skips_creating_it() {
        let dir = tempfile::tempdir().unwrap();
        temp_env(
            &[
                ("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap()),
                ("OPEN_RALLY_PASSWORD", "from-env"),
            ],
            || {
                assert_eq!(load().unwrap().password, "from-env");
                assert!(!config_path(dir.path()).exists());
                assert!(uploads_dir(dir.path()).is_dir());
            },
        );
        std::fs::write(dir.path().join("config.toml"), "password = \"file\"\n").unwrap();
        temp_env(
            &[
                ("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap()),
                ("OPEN_RALLY_PASSWORD", "from-env"),
            ],
            || assert_eq!(load().unwrap().password, "from-env"),
        );
        temp_env(
            &[
                ("OPEN_RALLY_DATA_DIR", dir.path().to_str().unwrap()),
                ("OPEN_RALLY_PASSWORD", ""),
            ],
            || assert_eq!(load().unwrap().password, "file"),
        );
    }

    #[test]
    fn retention_days_from_file_env_and_setter() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        temp_env(&[("OPEN_RALLY_DATA_DIR", d)], || {
            assert_eq!(load().unwrap().retention_days, 0, "default off");
            set_retention_days(dir.path(), 7).unwrap();
            let cfg = load().unwrap();
            assert_eq!(cfg.retention_days, 7);
            assert_eq!(cfg.password.len(), 12, "password preserved");
            let text = std::fs::read_to_string(config_path(dir.path())).unwrap();
            assert!(text.contains("retention_days = 7"), "{text}");
            set_retention_days(dir.path(), 0).unwrap();
            assert_eq!(load().unwrap().retention_days, 0);
        });
        temp_env(&[("OPEN_RALLY_DATA_DIR", d), ("OPEN_RALLY_RETENTION_DAYS", "30")], || {
            assert_eq!(load().unwrap().retention_days, 30);
        });
        temp_env(&[("OPEN_RALLY_DATA_DIR", d), ("OPEN_RALLY_RETENTION_DAYS", "x")], || {
            assert!(load().is_err());
        });
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
