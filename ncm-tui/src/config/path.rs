use std::fs;
use std::path::PathBuf;

const APP_NAME: &str = "ncm-tui-player";

#[allow(unused)]
pub struct Path {
    // 一级目录
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,

    // 二级目录
    pub api_program: PathBuf,
    pub settings: PathBuf,
    pub login_cookie: PathBuf,
    pub lyrics: PathBuf,
    pub daily_recommend: PathBuf,
    pub downloads: PathBuf,
}

impl Path {
    pub fn new() -> Self {
        let data = dirs_next::data_dir().unwrap().join(APP_NAME);
        if !data.exists() {
            fs::create_dir_all(&data).expect("Couldn't create data dir.");
        }

        let config = dirs_next::config_dir().unwrap().join(APP_NAME);
        if !config.exists() {
            fs::create_dir_all(&config).expect("Couldn't create config dir.");
        }

        let cache = dirs_next::cache_dir().unwrap().join(APP_NAME);
        if !cache.exists() {
            fs::create_dir_all(&cache).expect("Couldn't create cache dir.");
        }

        let api_program = data.clone().join("neteasecloudmusicapi");

        // settings.json 位于 config 目录；旧位置（data 目录）文件自动迁移
        let settings = config.clone().join("settings.json");
        let old_settings = data.clone().join("settings.json");
        if !settings.exists() && old_settings.exists() {
            let migrated = fs::rename(&old_settings, &settings).is_ok()
                // 跨文件系统等 rename 失败时退化为 copy + 删除
                || (fs::copy(&old_settings, &settings).is_ok() && fs::remove_file(&old_settings).is_ok());
            if !migrated {
                eprintln!("warning: failed to migrate settings.json to {:?}, starting with defaults", settings);
            }
        }

        let login_cookie = data.clone().join("cookies");

        let lyrics = cache.clone().join("lyrics");
        if !lyrics.exists() {
            fs::create_dir_all(&lyrics).expect("Couldn't create lyrics dir.");
        }

        let daily_recommend = cache.clone().join("daily_recommend");
        if !daily_recommend.exists() {
            fs::create_dir_all(&daily_recommend).expect("Couldn't create daily recommend dir.");
        }

        let downloads = data.clone().join("downloads");

        Self {
            data,
            config,
            cache,
            api_program,
            settings,
            login_cookie,
            lyrics,
            daily_recommend,
            downloads,
        }
    }
}
