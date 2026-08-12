pub mod model;
mod responses;
pub mod settings;

use crate::model::{Account, FromJson, LyricLine, Lyrics, Song, Songlist};
use crate::responses::login::*;
use crate::settings::{Settings, DOWNLOAD_QUALITIES};
use anyhow::{anyhow, Result};
use chrono::{Local, NaiveDate, NaiveTime, Utc};
use log::{debug, error};
use regex::Regex;
use reqwest::{Client, ClientBuilder};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::process;

pub struct NcmClient {
    api_program_path: PathBuf,
    cookie_path: PathBuf,
    lyrics_path: PathBuf,
    daily_recommend_path: PathBuf,
    settings_path: PathBuf,
    default_download_path: PathBuf,

    api_child_process: Option<process::Child>,
    http_client: Client,
    api_url: String,
    cookie: String,
    settings: Settings,

    login_account: Option<Account>,
    liked_song_ids: HashSet<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadResult {
    Downloaded(PathBuf),
    AlreadyExists(PathBuf),
}

struct SongResource {
    url: String,
    quality_level: String,
    file_type: Option<String>,
}

impl NcmClient {
    pub fn new(
        api_program_path: PathBuf,
        cookie_path: PathBuf,
        lyrics_path: PathBuf,
        daily_recommend_path: PathBuf,
        settings_path: PathBuf,
        default_download_path: PathBuf,
    ) -> Self {
        Self {
            api_program_path,
            cookie_path,
            lyrics_path,
            daily_recommend_path,
            settings_path,
            default_download_path,
            api_child_process: None,
            api_url: String::new(),
            http_client: ClientBuilder::new().no_proxy().build().expect("failed to build HTTP client"),
            cookie: String::new(),
            settings: Settings::default(),
            login_account: None,
            liked_song_ids: HashSet::new(),
        }
    }

    /// 初始化，尝试读取本地设置文件
    pub fn init(&mut self) {
        self.settings = self.read_settings();
        self.normalize_settings();

        // 更新（应对本地无设置文件或Settings数据结构更新的情况）
        self.store_settings();
    }

    /// 获取当前设置（副本）
    pub fn settings(&self) -> Settings {
        self.settings.clone()
    }

    /// 更新设置：兜底补全并写盘，返回 API 相关字段是否变化（变化需重跑 check_api）
    pub fn update_settings(&mut self, new: Settings) -> bool {
        let api_changed =
            self.settings.use_remote_api != new.use_remote_api || self.settings.remote_api_url != new.remote_api_url;
        self.settings = new;
        self.normalize_settings();
        self.store_settings();
        api_changed
    }

    /// 设置项为空时填回默认值，并确保下载目录存在
    fn normalize_settings(&mut self) {
        if self.settings.download_path.as_os_str().is_empty() {
            self.settings.download_path = self.default_download_path.clone();
        }
        if self.settings.download_file_name_pattern.is_empty() {
            self.settings.download_file_name_pattern = Settings::default().download_file_name_pattern;
        }
        if self.settings.download_lyric_name_pattern.is_empty() {
            self.settings.download_lyric_name_pattern = Settings::default().download_lyric_name_pattern;
        }
        if let Err(err) = fs::create_dir_all(&self.settings.download_path) {
            error!("failed to create download dir {:?}: {}", self.settings.download_path, err);
        }
    }

    /// 读取设置（读不到则返回默认设置）
    fn read_settings(&mut self) -> Settings {
        let mut settings = Settings::default();

        match File::open(&self.settings_path) {
            Ok(mut settings_file) => {
                let mut settings_json = String::new();
                if matches!(settings_file.read_to_string(&mut settings_json), Ok(_)) {
                    match serde_json::from_str(&settings_json) {
                        Ok(s) => {
                            settings = s;
                            debug!("read settings: {:?}", settings);
                        },
                        Err(err) => error!("failed to serialize settings from json: {:?}", err),
                    }
                }
            },
            Err(err) => error!("failed to read settings file, try to generate one later: {:?}", err),
        }

        settings
    }

    /// 保存设置
    pub fn store_settings(&mut self) {
        match serde_json::to_string_pretty(&self.settings) {
            Ok(settings_json) => match fs::OpenOptions::new().write(true).create(true).truncate(true).open(&self.settings_path) {
                Ok(mut settings_file) => match settings_file.write_all(settings_json.as_bytes()) {
                    Ok(_) => debug!("settings stored: {}", settings_json),
                    Err(err) => error!("failed to store settings {:?}", err),
                },
                Err(err) => error!("{:?}", err),
            },
            Err(err) => error!("failed to serialize settings from json: {:?}", err),
        }
    }

    /// 支持 local api 和 remote api
    ///
    /// local api 依赖本地 `~/.local/share/ncm-tui-player/neteasecloudmusicapi/` 的程序
    ///
    /// remote api 依赖部署在服务器的 `neteasecloudmusicapi` 程序
    pub async fn check_api(&mut self) -> bool {
        if self.settings.use_remote_api {
            self.check_remote_api().await
        } else {
            self.check_local_api().await
        }
    }

    /// 检查与 remote api 的连接性
    ///
    /// 若失败则会尝试 local api
    async fn check_remote_api(&mut self) -> bool {
        self.api_url = self.settings.remote_api_url.clone();

        if let Ok(response) = self.http_client.get(&self.api_url).send().await {
            if response.status().is_success() {
                debug!("api check passed");
                return true;
            }
        }

        self.check_local_api().await
    }

    /// 启动本地 api 程序，并检查连接性
    ///
    /// 将 nodejs 编写的 api 程序作为子进程启动，输出重定向到 stderr
    async fn check_local_api(&mut self) -> bool {
        self.api_url = String::from("http://localhost:3000");

        let api_program_path = self.api_program_path.to_str().unwrap();

        #[cfg(target_os = "linux")]
        let api_child_process: process::Child = process::Command::new("sh")
            .arg("-c")
            .arg(format!("node {}/app.js 1>&2", api_program_path))
            .spawn()
            .expect("Failed to spawn API child process on Linux");

        #[cfg(target_os = "windows")]
        let api_child_process: process::Child = process::Command::new("cmd")
            .arg("/C")
            .arg(format!("node {}/app.js > {}/api.log 2>&1", api_program_path, api_program_path))
            .spawn()
            .expect("Failed to spawn API child process on Windows");

        #[cfg(target_os = "macos")]
        // TODO: macos 下的命令待修正
        let api_child_process: process::Child = process::Command::new("sh")
            .arg("-c")
            .arg(format!("node {}/app.js 1>&2", api_program_path))
            .spawn()
            .expect("Failed to spawn API child process on MacOS");

        self.api_child_process = Some(api_child_process);

        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            if let Ok(response) = self.http_client.get(&self.api_url).send().await {
                if response.status().is_success() {
                    debug!("api check passed");
                    return true;
                }
            }
        }

        false
    }

    /// 退出客户端时，终止 api 子进程
    pub async fn exit_client(&mut self) -> Result<()> {
        match self.api_child_process.as_mut() {
            Some(api_child_process) => {
                api_child_process.kill().await?;
                api_child_process.wait().await?;
            },
            None => {},
        }

        Ok(())
    }
}

// 登录 api
impl NcmClient {
    /// 保存 cookie
    pub fn store_cookie(&self) {
        match fs::OpenOptions::new().write(true).create(true).truncate(true).open(&self.cookie_path) {
            Ok(mut cookie_file) => match cookie_file.write_all(self.cookie.clone().as_bytes()) {
                Ok(_) => debug!("cookie stored at {:?}", &self.cookie_path),
                Err(err) => error!("failed to store cookie at {:?}: {}", &self.cookie_path, err),
            },
            Err(err) => error!("{:?}", err),
        }
    }

    /// 读 cookie
    fn read_cookie(&mut self) {
        match File::open(&self.cookie_path) {
            Ok(mut cookie_file) => match cookie_file.read_to_string(&mut self.cookie) {
                Ok(_) => debug!("read cookie: {}", &self.cookie),
                Err(err) => error!("failed to read cookie at {:?}: {}", &self.cookie_path, err),
            },
            Err(err) => error!("failed to open cookie at {:?}: {}", &self.cookie_path, err),
        }
    }

    /// 尝试从本地读取 cookie 登录
    pub async fn try_cookie_login(&mut self) -> Result<bool> {
        self.read_cookie();
        if self.cookie.is_empty() {
            return Ok(false);
        }

        self.check_login_status().await?;
        if let Some(_) = self.login_account.as_ref() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 获取登录二维码 (uni_key, url)
    pub async fn get_login_qr(&self) -> Result<(String, String)> {
        let key_response = self
            .http_client
            .get(format!("{}/login/qr/key?timestamp={}", &self.api_url, Utc::now().timestamp()))
            .send()
            .await?
            .json::<QrResponse<QrKeyData>>()
            .await?;

        if key_response.code == 200 && key_response.data.code == 200 {
            let uni_key = key_response.data.unikey;

            let create_response = self
                .http_client
                .get(format!("{}/login/qr/create?key={}&qrimg=true&timestamp={}", &self.api_url, &uni_key, Utc::now().timestamp()))
                .send()
                .await?
                .json::<QrResponse<QrCreateData>>()
                .await?;

            if create_response.code == 200 {
                debug!("get login qr key & url: {}, {}", uni_key, create_response.data.qrurl);
                Ok((uni_key, create_response.data.qrurl))
            } else {
                Err(anyhow!("failed to get login qr url"))
            }
        } else {
            Err(anyhow!("failed to get login qr unikey"))
        }
    }

    /// 检查登录二维码状态
    pub async fn check_login_qr(&mut self, uni_key: &str) -> Result<usize> {
        let check_response = self
            .http_client
            .get(format!("{}/login/qr/check?key={}&timestamp={}", &self.api_url, &uni_key, Utc::now().timestamp()))
            .send()
            .await?
            .json::<QrCheckResponse>()
            .await?;

        debug!("check login qr status: {}", check_response.code);

        // 登录成功
        if check_response.code == 803 {
            self.cookie = check_response.cookie;
        }

        Ok(check_response.code)
    }

    /// 获取登录状态
    pub async fn check_login_status(&mut self) -> Result<()> {
        let status_response = self
            .http_client
            .post(format!("{}/login/status", &self.api_url))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?
            .bytes()
            .await?;

        let mut v: Value = serde_json::from_slice(&status_response)?;
        let v_profile = v["data"]["profile"].take();
        if !v_profile.is_null() {
            if let Ok(account) = Account::from_json(v_profile) {
                debug!("login, {:?}", account);
                self.login_account = Some(account);
            }
        }

        Ok(())
    }

    /// 是否登录
    pub fn is_login(&self) -> bool {
        if let Some(_) = self.login_account {
            true
        } else {
            false
        }
    }

    /// 登录的账号信息
    pub fn login_account(&self) -> Option<Account> {
        self.login_account.clone()
    }

    /// 登出
    pub async fn logout(&mut self) -> Result<()> {
        // TODO
        Ok(())
    }
}

// 用户 api
impl NcmClient {
    /// 加载用户喜欢的歌曲 ID
    pub async fn load_liked_song_ids(&mut self) -> Result<()> {
        let user_id = self.login_account.as_ref().ok_or_else(|| anyhow!("not logged in"))?.user_id;
        let response = self
            .http_client
            .post(format!(
                "{}/likelist?uid={}&timestamp={}",
                self.api_url,
                user_id,
                Utc::now().timestamp_millis()
            ))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        if response["code"].as_u64() != Some(200) {
            return Err(anyhow!("failed to load liked songs, code {:?}", response["code"]));
        }

        self.liked_song_ids = response["ids"]
            .as_array()
            .ok_or_else(|| anyhow!("liked song list is missing ids"))?
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        Ok(())
    }

    pub fn is_song_liked(&self, song_id: u64) -> bool {
        self.liked_song_ids.contains(&song_id)
    }
}

// 歌单 api
impl NcmClient {
    /// 获取用户所有歌单（创建的+收藏的）
    pub async fn get_user_all_songlists(&self) -> Result<Vec<Songlist>> {
        let mut songlists: Vec<Songlist> = Vec::new();

        if let Some(login_account) = self.login_account.as_ref() {
            let user_id = login_account.user_id;

            let playlist_response = self
                .http_client
                .post(format!("{}/user/playlist?uid={}", &self.api_url, user_id))
                .form(&[("cookie", &self.cookie)])
                .send()
                .await?;

            let v_playlist: Value = serde_json::from_slice(&playlist_response.bytes().await?)?;

            // 状态码报错
            if v_playlist["code"].as_u64().unwrap() != 200 {
                return Err(anyhow!("failed to load songs into songlist, code {}", v_playlist["code"].as_u64().unwrap()));
            }
            // 仍有更多页
            if v_playlist["more"].as_bool().unwrap() {
                // TODO: 增加 offset ，继续获取
            }

            for playlist in v_playlist["playlist"].as_array().unwrap() {
                songlists.push(Songlist {
                    name: playlist["name"].as_str().unwrap().to_string(),
                    id: playlist["id"].as_u64().unwrap(),
                    songs_count: playlist["trackCount"].as_u64().unwrap_or(0) as usize,
                    creator: if let Some(creator_nickname) = playlist["creator"]["nickname"].as_str() {
                        creator_nickname.to_string()
                    } else {
                        String::new()
                    },
                    subscribed: playlist["subscribed"].as_bool().unwrap_or(false),
                    special_type: playlist["specialType"].as_u64().unwrap_or(0),
                    songs: Vec::new(),
                });
            }

            debug!("songlists: {:?}", songlists);
        }

        Ok(songlists)
    }

    /// 装载歌单内的所有歌曲
    pub async fn load_songlist_songs(&self, songlist: &mut Songlist) -> Result<()> {
        songlist.songs = request_songlist_songs(&self.http_client, &self.api_url, &self.cookie, &self.liked_song_ids, songlist.id).await?;
        debug!("{:?}", songlist.songs);
        Ok(())
    }

    /// 创建不借用 NcmClient 的歌单加载 future，供后台下载使用。
    pub fn load_songlist(&self, mut songlist: Songlist) -> impl std::future::Future<Output = Result<Songlist>> + Send + 'static {
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();
        let cookie = self.cookie.clone();
        let liked_song_ids = self.liked_song_ids.clone();

        async move {
            songlist.songs = request_songlist_songs(&http_client, &api_url, &cookie, &liked_song_ids, songlist.id).await?;
            Ok(songlist)
        }
    }

    /// 向歌单添加/移除歌曲（仅对自建歌单有效）
    pub async fn update_songlist_tracks(&self, add: bool, songlist_id: u64, song_id: u64) -> Result<()> {
        let response = self
            .http_client
            .post(format!(
                "{}/playlist/tracks?op={}&pid={}&tracks={}",
                self.api_url,
                if add { "add" } else { "del" },
                songlist_id,
                song_id
            ))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        // 该端点的响应被包裹为 {"status": 200, "body": {"code": 200, ...}}，兼容顶层 code
        let code = response["body"]["code"].as_u64().or_else(|| response["code"].as_u64());
        if code == Some(200) {
            Ok(())
        } else {
            Err(anyhow!(
                "failed to {} song {} in songlist {}, code {:?}: {}",
                if add { "add" } else { "del" },
                song_id,
                songlist_id,
                code,
                response["body"]["message"].as_str().or_else(|| response["message"].as_str()).unwrap_or("unknown error")
            ))
        }
    }

    /// 创建歌单（默认私有），返回新歌单 id（响应结构不确定时返回 None）
    pub async fn create_songlist(&self, name: &str) -> Result<Option<u64>> {
        let response = self
            .http_client
            .post(format!("{}/playlist/create", self.api_url))
            .query(&[("name", name), ("privacy", "10")])
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        if response["code"].as_u64() == Some(200) {
            Ok(response["playlist"]["id"].as_u64().or_else(|| response["id"].as_u64()))
        } else {
            Err(anyhow!("failed to create songlist {}, code {:?}", name, response["code"]))
        }
    }

    /// 删除自建歌单
    pub async fn delete_songlist(&self, songlist_id: u64) -> Result<()> {
        let response = self
            .http_client
            .post(format!("{}/playlist/delete?id={}", self.api_url, songlist_id))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        if response["code"].as_u64() == Some(200) {
            Ok(())
        } else {
            Err(anyhow!("failed to delete songlist {}, code {:?}", songlist_id, response["code"]))
        }
    }
}

// 歌曲 api
impl NcmClient {
    /// 喜欢或取消喜欢歌曲
    pub async fn like_song(&mut self, song_id: u64, like: bool) -> Result<()> {
        let response = self
            .http_client
            .post(format!(
                "{}/like?id={}&like={}&timestamp={}",
                self.api_url,
                song_id,
                like,
                Utc::now().timestamp_millis()
            ))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        if response["code"].as_u64() == Some(200) {
            if like {
                self.liked_song_ids.insert(song_id);
            } else {
                self.liked_song_ids.remove(&song_id);
            }
            debug!("set liked={} for song {}", like, song_id);
            Ok(())
        } else {
            Err(anyhow!(
                "failed to set liked={} for song {}, code {:?}: {}",
                like,
                song_id,
                response["code"],
                response["message"].as_str().unwrap_or("unknown error")
            ))
        }
    }

    /// 检查歌曲是否可获取
    pub async fn check_song_availability(&self, song_id: u64) -> Result<bool> {
        let check_response = self
            .http_client
            .post(format!("{}/check/music?id={}", &self.api_url, song_id))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let v_check_response: Value = serde_json::from_slice(&check_response.bytes().await?)?;

        if v_check_response["code"].as_u64().unwrap() == 200 {
            return Ok(v_check_response["success"].as_bool().unwrap_or(false));
        }

        Ok(false)
    }

    /// 上报完整播放记录
    pub async fn scrobble(&self, song_id: u64, source_id: u64, time: u64) -> Result<()> {
        let response = self
            .http_client
            .post(format!(
                "{}/scrobble?id={}&sourceid={}&time={}&timestamp={}",
                self.api_url,
                song_id,
                source_id,
                time,
                Utc::now().timestamp_millis()
            ))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let response: Value = serde_json::from_slice(&response.bytes().await?)?;
        if response["code"].as_u64() == Some(200) {
            debug!("scrobble request accepted for song {} from source {}", song_id, source_id);
            Ok(())
        } else {
            Err(anyhow!("failed to scrobble song, code {:?}", response["code"]))
        }
    }

    /// 装载歌曲 url
    pub async fn load_song_url(&self, song: &mut Song) -> Result<()> {
        song.song_url = None;

        let quality = &self.settings.play_quality;
        validate_quality(quality)?;
        let resource = request_song_resource(&self.http_client, &self.api_url, &self.cookie, song.id, quality).await?;
        song.song_url = Some(resource.url);
        song.quality_level = quality_level_name(&resource.quality_level);

        Ok(())
    }

    /// 创建不借用 NcmClient 的下载 future，避免下载期间持有全局锁。
    pub fn download_song(&self, song: Song) -> impl std::future::Future<Output = Result<DownloadResult>> + Send + 'static {
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();
        let cookie = self.cookie.clone();
        let download_path = self.settings.download_path.clone();
        let quality = self.settings.download_quality.clone();
        let name_pattern = self.settings.download_file_name_pattern.clone();
        let lyric_name_pattern = self.settings.download_lyric_name_pattern.clone();

        async move {
            validate_quality(&quality)?;

            let result = if let Some(path) = find_local_song_with_quality(&download_path, song.id, &quality) {
                DownloadResult::AlreadyExists(path)
            } else {
                let resource = request_song_resource(&http_client, &api_url, &cookie, song.id, &quality).await?;
                let file_type = resource
                    .file_type
                    .filter(|file_type| !file_type.is_empty() && file_type.chars().all(|c| c.is_ascii_alphanumeric()))
                    .ok_or_else(|| anyhow!("song {} response has no valid file type", song.id))?;

                tokio::fs::create_dir_all(&download_path).await?;
                let file_name = build_download_file_name(&song, &quality, &file_type, &name_pattern);
                let final_path = download_path.join(file_name);
                if final_path.is_file() {
                    DownloadResult::AlreadyExists(final_path)
                } else {
                    let part_path = final_path.with_extension(format!("{}.part", file_type));

                    let mut response = http_client.get(resource.url).send().await?.error_for_status()?;
                    let mut file = tokio::fs::File::create(&part_path).await?;
                    let mut written = 0;
                    while let Some(chunk) = response.chunk().await? {
                        file.write_all(&chunk).await?;
                        written += chunk.len();
                    }
                    if written == 0 {
                        return Err(anyhow!("song {} download returned an empty file", song.id));
                    }
                    file.flush().await?;
                    drop(file);
                    tokio::fs::rename(&part_path, &final_path).await?;

                    DownloadResult::Downloaded(final_path)
                }
            };

            // 歌词为附加产物，失败不影响歌曲下载结果
            save_lyric_file(&http_client, &api_url, &cookie, &download_path, &song, &quality, &lyric_name_pattern).await;
            Ok(result)
        }
    }

    pub fn find_local_song(&self, song_id: u64) -> Option<PathBuf> {
        find_local_song_file(&self.settings.download_path, song_id, Some(&self.settings.download_quality))
    }

    /// 下载目录中已下载歌曲的 ID 集合（忽略 .part 未完成文件）
    pub fn downloaded_song_ids(&self) -> HashSet<u64> {
        downloaded_song_ids_in(&self.settings.download_path)
    }

    /// 日推窗口内的日期（当日 6 点生成，6 点前窗口整体后移一天）
    pub fn daily_recommend_window(&self) -> Vec<NaiveDate> {
        daily_recommend_window_at(Local::now())
    }

    /// 获取某日每日推荐歌曲（优先读本地缓存；某日推生成后不再变化，缓存按日期永久有效）
    pub async fn get_daily_recommend_songs(&self, date: NaiveDate) -> Result<Vec<Song>> {
        let cache_file = self.daily_recommend_path.join(format!("{}.json", date.format("%Y-%m-%d")));

        if let Ok(mut cache) = File::open(&cache_file) {
            let mut json = String::new();
            cache.read_to_string(&mut json)?;
            let songs: Vec<Song> = serde_json::from_str(&json)?;
            // 空列表视为未命中（历史上可能缓存过接口限制导致的空结果），转由在线拉取自愈
            if !songs.is_empty() {
                return Ok(songs);
            }
        }

        let latest = self.daily_recommend_window()[0];
        let songs = if date == latest {
            request_daily_recommend_songs(&self.http_client, &self.api_url, &self.cookie, &self.liked_song_ids).await?
        } else {
            request_history_daily_recommend_songs(&self.http_client, &self.api_url, &self.cookie, &self.liked_song_ids, date).await?
        };

        // 缓存写入失败不影响本次返回
        match serde_json::to_string(&songs) {
            Ok(json) => {
                if let Err(err) = fs::write(&cache_file, json) {
                    error!("failed to store daily recommend cache {:?}: {:?}", cache_file, err);
                }
            },
            Err(err) => error!("failed to serialize daily recommend songs: {:?}", err),
        }

        Ok(songs)
    }

    /// 删除窗口外的日推缓存文件
    pub fn purge_daily_recommend_cache(&self) {
        purge_daily_recommend_cache(&self.daily_recommend_path, &self.daily_recommend_window());
    }

    /// 获取歌曲的歌词
    pub async fn get_song_lyrics(&self, song_id: u64) -> Result<Lyrics> {
        // 优先尝试从本地缓存读取歌词
        if let Ok(lyrics) = self.try_read_lyrics_cache(song_id) {
            return Ok(lyrics);
        }

        let lyric_response = self
            .http_client
            .post(format!("{}/lyric?id={}", &self.api_url, song_id))
            .form(&[("cookie", &self.cookie)])
            .send()
            .await?;

        let v_lyric: Value = serde_json::from_slice(&lyric_response.bytes().await?)?;

        let lyric_text = v_lyric["lrc"]["lyric"].as_str().unwrap_or("").to_string();
        let trans_lyric_text = v_lyric["tlyric"]["lyric"].as_str().unwrap_or("").to_string();
        let roman_lyric_text = v_lyric["romalrc"]["lyric"].as_str().unwrap_or("").to_string();

        let origin_lyric_lines: Vec<String> = lyric_text.split('\n').into_iter().map(|s| s.to_string()).collect();
        let origin_trans_lyric_lines: Vec<String> = trans_lyric_text.split('\n').into_iter().map(|s| s.to_string()).collect();
        let origin_roman_lyric_lines: Vec<String> = roman_lyric_text.split('\n').into_iter().map(|s| s.to_string()).collect();

        // 编码歌词
        let lyrics = encode_lyrics(origin_lyric_lines, origin_trans_lyric_lines, origin_roman_lyric_lines);

        debug!("lyrics encoded: {:?}", lyrics);

        // 将歌词缓存到本地
        self.store_lyrics_cache(song_id, &lyrics);

        Ok(lyrics)
    }

    /// 缓存歌词
    fn store_lyrics_cache(&self, song_id: u64, lyrics: &Lyrics) {
        match serde_json::to_string(lyrics) {
            Ok(lyrics_json) => match fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(self.lyrics_path.clone().join(format!("{}.lyrics", song_id)))
            {
                Ok(mut lyrics_file) => match lyrics_file.write_all(lyrics_json.as_bytes()) {
                    Ok(_) => debug!("lyrics stored at {:?}", &self.lyrics_path),
                    Err(err) => {
                        error!("failed to store lyrics at {:?}: {:?}", &self.lyrics_path, err)
                    },
                },
                Err(err) => error!("{:?}", err),
            },
            Err(err) => error!("{:?}", err),
        }
    }

    /// 尝试读本地歌词缓存
    fn try_read_lyrics_cache(&self, song_id: u64) -> Result<Lyrics> {
        let mut lyrics_file = File::open(self.lyrics_path.clone().join(format!("{}.lyrics", song_id)))?;
        let mut json_data = String::new();
        lyrics_file.read_to_string(&mut json_data)?;
        let lyrics: Lyrics = serde_json::from_str(&json_data)?;
        debug!("read lyrics from cache: {:?}", lyrics);

        Ok(lyrics)
    }
}

/// 日推窗口天数
const DAILY_RECOMMEND_DAYS: i64 = 14;

/// 计算日推窗口：当日日推 6 点生成，6 点前窗口整体后移一天
fn daily_recommend_window_at(now: chrono::DateTime<Local>) -> Vec<NaiveDate> {
    let six_am = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
    let today = now.date_naive();
    let latest = if now.time() < six_am { today - chrono::Duration::days(1) } else { today };
    (0..DAILY_RECOMMEND_DAYS).map(|i| latest - chrono::Duration::days(i)).collect()
}

/// 删除窗口外日期的日推缓存文件
fn purge_daily_recommend_cache(dir: &Path, window: &[NaiveDate]) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let expired = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok())
                .is_some_and(|date| !window.contains(&date));
            if expired {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// 请求当日每日推荐歌曲
async fn request_daily_recommend_songs(client: &Client, api_url: &str, cookie: &str, liked_song_ids: &HashSet<u64>) -> Result<Vec<Song>> {
    let response = client
        .post(format!("{}/recommend/songs", api_url))
        .form(&[("cookie", cookie)])
        .send()
        .await?
        .error_for_status()?;
    let value: Value = serde_json::from_slice(&response.bytes().await?)?;
    if value["code"].as_u64() != Some(200) {
        return Err(anyhow!("failed to load daily recommend songs, code {:?}", value["code"]));
    }
    let tracks = value["data"]["dailySongs"].as_array().ok_or_else(|| anyhow!("daily recommend response contains no songs"))?;
    Ok(tracks.iter().filter_map(|track| parse_track(track, liked_song_ids)).collect())
}

/// 请求历史某日每日推荐歌曲
async fn request_history_daily_recommend_songs(client: &Client, api_url: &str, cookie: &str, liked_song_ids: &HashSet<u64>, date: NaiveDate) -> Result<Vec<Song>> {
    let response = client
        .post(format!("{}/history/recommend/songs/detail?date={}", api_url, date.format("%Y-%m-%d")))
        .form(&[("cookie", cookie)])
        .send()
        .await?
        .error_for_status()?;
    let value: Value = serde_json::from_slice(&response.bytes().await?)?;
    if value["code"].as_u64() != Some(200) {
        return Err(anyhow!("failed to load history daily recommend songs, code {:?}", value["code"]));
    }
    // 该接口响应结构未文档化，兼容 data 直接为歌曲数组与 data.songs 两种形态
    let tracks = value["data"]
        .as_array()
        .or_else(|| value["data"]["songs"].as_array())
        .ok_or_else(|| {
            let raw = value.to_string();
            anyhow!("history daily recommend response contains no songs: {:.200}", raw)
        })?;
    // 空列表是当天未生成日推记录的信号，该日期无法回溯
    if tracks.is_empty() {
        return Err(anyhow!("历史记录无缓存"));
    }
    Ok(tracks.iter().filter_map(|track| parse_track(track, liked_song_ids)).collect())
}

/// 解析服务端 track JSON 为 Song（无 id 的曲目返回 None）
fn parse_track(track: &Value, liked_song_ids: &HashSet<u64>) -> Option<Song> {
    let id = track["id"].as_u64()?;
    Some(Song {
        name: track["name"].as_str().unwrap_or("Unknown").to_string(),
        id,
        singer: track["ar"][0]["name"].as_str().unwrap_or("Unknown").to_string(),
        singer_id: track["ar"][0]["id"].as_u64().unwrap_or(0),
        album: track["al"]["name"].as_str().unwrap_or("Unknown").to_string(),
        album_id: track["al"]["id"].as_u64().unwrap_or(0),
        duration: track["dt"].as_u64().unwrap_or(0),
        song_url: None,
        quality_level: String::new(),
        liked: liked_song_ids.contains(&id),
    })
}

async fn request_songlist_songs(client: &Client, api_url: &str, cookie: &str, liked_song_ids: &HashSet<u64>, songlist_id: u64) -> Result<Vec<Song>> {
    let mut result = Vec::new();
    let mut offset = 0;

    loop {
        let response = client
            .post(format!("{}/playlist/track/all?id={}&limit=1000&offset={}", api_url, songlist_id, offset))
            .form(&[("cookie", cookie)])
            .send()
            .await?
            .error_for_status()?;
        let value: Value = serde_json::from_slice(&response.bytes().await?)?;
        if value["code"].as_u64() != Some(200) {
            return Err(anyhow!("failed to load songs into songlist, code {:?}", value["code"]));
        }
        let tracks = value["songs"].as_array().ok_or_else(|| anyhow!("songlist {} response contains no songs", songlist_id))?;
        if tracks.is_empty() {
            break;
        }

        for track in tracks {
            result.push(parse_track(track, liked_song_ids).ok_or_else(|| anyhow!("songlist {} contains a song without id", songlist_id))?);
        }
        if tracks.len() < 1000 {
            break;
        }
        offset += 1000;
    }
    Ok(result)
}

async fn request_song_resource(client: &Client, api_url: &str, cookie: &str, song_id: u64, quality: &str) -> Result<SongResource> {
    let response = client
        .post(format!("{}/song/url/v1?id={}&level={}", api_url, song_id, quality))
        .form(&[("cookie", cookie)])
        .send()
        .await?
        .error_for_status()?;
    let value: Value = serde_json::from_slice(&response.bytes().await?)?;
    let data = value["data"].as_array().and_then(|data| data.first()).ok_or_else(|| anyhow!("song {} response contains no data", song_id))?;
    let url = data["url"].as_str().ok_or_else(|| anyhow!("song {} is unavailable for download", song_id))?.to_string();

    Ok(SongResource {
        url,
        quality_level: data["level"].as_str().unwrap_or(quality).to_string(),
        file_type: data["type"].as_str().map(str::to_string),
    })
}

async fn request_lyric_text(client: &Client, api_url: &str, cookie: &str, song_id: u64) -> Result<Option<String>> {
    let response = client
        .post(format!("{}/lyric?id={}", api_url, song_id))
        .form(&[("cookie", cookie)])
        .send()
        .await?
        .error_for_status()?;
    let value: Value = serde_json::from_slice(&response.bytes().await?)?;
    let text = value["lrc"]["lyric"].as_str().unwrap_or("").to_string();
    Ok((!text.trim().is_empty()).then_some(text))
}

async fn save_lyric_file(client: &Client, api_url: &str, cookie: &str, download_path: &Path, song: &Song, quality: &str, name_pattern: &str) {
    let lyric_path = download_path.join(format!("{}.lrc", render_name_pattern(song, quality, name_pattern)));
    if lyric_path.is_file() {
        return;
    }
    match request_lyric_text(client, api_url, cookie, song.id).await {
        Ok(Some(text)) => {
            if let Err(err) = tokio::fs::write(&lyric_path, text).await {
                error!("failed to store lyric at {:?}: {}", lyric_path, err);
            }
        },
        Ok(None) => debug!("song {} has no lyric, skip storing lyric file", song.id),
        Err(err) => error!("failed to fetch lyric for song {}: {}", song.id, err),
    }
}

fn validate_quality(quality: &str) -> Result<()> {
    if DOWNLOAD_QUALITIES.contains(&quality) {
        Ok(())
    } else {
        Err(anyhow!("invalid quality '{}'; supported values: {}", quality, DOWNLOAD_QUALITIES.join(", ")))
    }
}

fn quality_level_name(quality: &str) -> String {
    match quality {
        "standard" => String::from("标准"),
        "higher" => String::from("较高"),
        "exhigh" => String::from("极高"),
        "lossless" => String::from("无损"),
        "hires" => String::from("Hi-Res"),
        "jyeffect" => String::from("高清环绕声"),
        "sky" => String::from("沉浸环绕声"),
        "dolby" => String::from("杜比全景声"),
        "jymaster" => String::from("超清母带"),
        _ => quality.to_string(),
    }
}

fn sanitize_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let sanitized = sanitized.trim_matches(|c| c == ' ' || c == '.');
    if sanitized.is_empty() { String::from("song") } else { sanitized.to_string() }
}

fn render_name_pattern(song: &Song, quality: &str, pattern: &str) -> String {
    let stem = pattern
        .replace("{name}", &sanitize_file_name(&song.name))
        .replace("{singer}", &sanitize_file_name(&song.singer))
        .replace("{album}", &sanitize_file_name(&song.album))
        .replace("{quality}", quality)
        .replace("{id}", &song.id.to_string());
    sanitize_file_name(&stem)
}

fn build_download_file_name(song: &Song, quality: &str, file_type: &str, pattern: &str) -> String {
    format!("{}.{}", render_name_pattern(song, quality, pattern), file_type)
}

// ponytail: 匹配依赖“id 在开头或结尾、quality 被 - 包裹”，默认/旧命名均满足
fn file_stem_matches(stem: &str, song_id: u64, quality: Option<&str>) -> bool {
    let id_matches = stem.ends_with(&format!("-{}", song_id)) || stem.starts_with(&format!("{}-", song_id));
    let quality_matches = quality.is_none_or(|quality| stem.contains(&format!("-{}-", quality)));
    id_matches && quality_matches
}

fn find_local_song_with_quality(download_path: &Path, song_id: u64, quality: &str) -> Option<PathBuf> {
    find_local_song_file(download_path, song_id, Some(quality))
}

fn find_local_song_file(download_path: &Path, song_id: u64, preferred_quality: Option<&str>) -> Option<PathBuf> {
    let stem_of = |path: &PathBuf| path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string);
    let mut paths: Vec<PathBuf> = fs::read_dir(download_path)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) != Some("part")
                && stem_of(path).is_some_and(|stem| file_stem_matches(&stem, song_id, None))
        })
        .collect();
    paths.sort();

    paths
        .iter()
        .find(|path| stem_of(path).is_some_and(|stem| file_stem_matches(&stem, song_id, preferred_quality)))
        .cloned()
        .or_else(|| paths.into_iter().next())
}

// ponytail: 仅按首/尾数字段解析 id，曲名自带 "-数字" 结尾且非本应用下载文件时可能误判，概率低
fn song_id_from_file_stem(stem: &str) -> Option<u64> {
    stem.rsplit('-').next().and_then(|s| s.parse().ok()).or_else(|| stem.split('-').next().and_then(|s| s.parse().ok()))
}

fn downloaded_song_ids_in(download_path: &Path) -> HashSet<u64> {
    let Ok(entries) = fs::read_dir(download_path) else { return HashSet::new() };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && path.extension().and_then(|extension| extension.to_str()) != Some("part"))
        .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()).and_then(song_id_from_file_stem))
        .collect()
}

#[inline]
/// 编码并序列化歌词
fn encode_lyrics(origin_lyric_lines: Vec<String>, origin_trans_lyric_lines: Vec<String>, origin_roman_lyric_lines: Vec<String>) -> Lyrics {
    let mut lyrics: Lyrics = Vec::new();

    // 正则表达式
    let timestamp_re = Regex::new(r"\[\d+:\d+.\d+]").unwrap(); // 时间戳
    let timestamp_abnormal_re = Regex::new(r"^\[(\d+):(\d+):(\d+)]").unwrap(); // 不正常时间戳
    let timestamp_9bit_re = Regex::new(r"\[(\d+):(\d+).(\d)]").unwrap(); // 9位时间戳（小数点后ms部分只有1位）
    let timestamp_10bit_re = Regex::new(r"\[(\d+):(\d+).(\d)(\d)]").unwrap(); // 10位时间戳（小数点后ms部分只有2位）
    let timestamp_7bit_re = Regex::new(r"\[(\d+):(\d+)]").unwrap(); // 7位时间戳（无小数点及ms部分）

    // 修正闭包
    let fix_line = |line: &String| -> String {
        let mut fixed = timestamp_7bit_re.replace_all(line, "[$1:$2.000]").to_string();
        fixed = timestamp_10bit_re.replace_all(&fixed, "[$1:$2.0$3$4]").to_string();
        fixed = timestamp_9bit_re.replace_all(&fixed, "[$1:$2.00$3]").to_string();
        fixed = timestamp_abnormal_re.replace_all(&fixed, "[$1:$2.$3]").to_string();
        fixed.to_string()
    };

    // 进行修正
    let fixed_lyric_lines: Vec<String> = origin_lyric_lines.iter().map(fix_line).collect();
    let fixed_trans_lyric_lines: Vec<String> = origin_trans_lyric_lines.iter().map(fix_line).collect();
    let fixed_roman_lyric_lines: Vec<String> = origin_roman_lyric_lines.iter().map(fix_line).collect();

    // 匹配时间戳并编码
    let mut trans_lyric_line_pointer = (fixed_trans_lyric_lines.len() - 1) as isize;
    let mut roman_lyric_line_pointer = (fixed_roman_lyric_lines.len() - 1) as isize;
    //
    for lyric_line in fixed_lyric_lines.iter().rev() {
        // lyric
        if timestamp_re.is_match(lyric_line) {
            // 计算时间戳
            let timestamp = (lyric_line[1..=2].parse::<u64>().unwrap() * 60 + lyric_line[4..=5].parse::<u64>().unwrap()) * 1000 + lyric_line[7..=9].parse::<u64>().unwrap_or(0);

            lyrics.push(LyricLine {
                timestamp,
                lyric_line: timestamp_re.replace_all(lyric_line, "").trim_end_matches('\t').to_string(),
                trans_lyric_line: None,
                roman_lyric_line: None,
            })
        } else {
            continue;
        }

        // trans_lyric
        while trans_lyric_line_pointer >= 0 {
            if let Some(trans_lyric_line) = fixed_trans_lyric_lines.get(trans_lyric_line_pointer as usize) {
                if !timestamp_re.is_match(trans_lyric_line) {
                    trans_lyric_line_pointer -= 1;
                    continue;
                }

                if trans_lyric_line.starts_with(&lyric_line[0..=10]) {
                    if let Some(last) = lyrics.last_mut() {
                        last.trans_lyric_line = Some(timestamp_re.replace_all(trans_lyric_line, "").trim_end_matches('\t').to_string());
                    }

                    trans_lyric_line_pointer -= 1;
                }

                break;
            } else {
                break;
            }
        }

        // roman_lyric
        while roman_lyric_line_pointer >= 0 {
            if let Some(roman_lyric_line) = fixed_roman_lyric_lines.get(roman_lyric_line_pointer as usize) {
                if !timestamp_re.is_match(roman_lyric_line) {
                    roman_lyric_line_pointer -= 1;
                    continue;
                }

                if roman_lyric_line.starts_with(&lyric_line[0..=10]) {
                    if let Some(last) = lyrics.last_mut() {
                        last.roman_lyric_line = Some(timestamp_re.replace_all(roman_lyric_line, "").trim_end_matches('\t').to_string());
                    }

                    roman_lyric_line_pointer -= 1;
                }

                break;
            } else {
                break;
            }
        }
    }

    lyrics.reverse();
    lyrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_dir() -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("ncm-api-test-{}-{}", std::process::id(), NEXT_ID.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn song(id: u64) -> Song {
        Song {
            name: String::from("Test Song"), id, singer: String::new(), singer_id: 0, album: String::new(), album_id: 0,
            duration: 0, song_url: None, quality_level: String::new(), liked: false,
        }
    }

    #[test]
    fn validates_download_quality() {
        for quality in DOWNLOAD_QUALITIES {
            assert!(validate_quality(quality).is_ok());
        }
        assert!(validate_quality("unknown").is_err());
    }

    #[test]
    fn sanitizes_cross_platform_file_names() {
        assert_eq!(sanitize_file_name(" A/\\:*?\"<>|\u{7f}. "), "A__________");
        assert_eq!(sanitize_file_name(" .. "), "song");
    }

    #[test]
    fn builds_download_file_name_from_pattern() {
        let song = Song { name: String::from("晴天"), singer: String::from("周杰伦"), album: String::from("叶惠美"), ..song(42) };
        assert_eq!(
            build_download_file_name(&song, "lossless", "flac", "{name}-{singer}-{album}-{quality}-{id}"),
            "晴天-周杰伦-叶惠美-lossless-42.flac"
        );
        assert_eq!(build_download_file_name(&song, "lossless", "flac", "{id}-{name}"), "42-晴天.flac");
        assert_eq!(render_name_pattern(&song, "lossless", "{name}-Lyric"), "晴天-Lyric");
        assert_eq!(render_name_pattern(&song, "lossless", "{name}-{id}-Lyric"), "晴天-42-Lyric");
    }

    #[test]
    fn collects_downloaded_song_ids_ignoring_parts() {
        let dir = test_dir();
        File::create(dir.join("晴天-周杰伦-叶惠美-lossless-42.flac")).unwrap();
        File::create(dir.join("43-jymaster-song.flac")).unwrap();
        File::create(dir.join("song-a-x-lossless-44.flac.part")).unwrap();

        let ids = downloaded_song_ids_in(&dir);
        assert!(ids.contains(&42));
        assert!(ids.contains(&43));
        assert!(!ids.contains(&44));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finds_preferred_quality_then_falls_back_and_ignores_parts() {
        let dir = test_dir();
        let fallback = dir.join("song-a-x-standard-42.mp3");
        let preferred = dir.join("song-a-x-lossless-42.flac");
        File::create(&fallback).unwrap();
        File::create(&preferred).unwrap();
        File::create(dir.join("song-a-x-lossless-42.flac.part")).unwrap();

        assert_eq!(find_local_song_file(&dir, 42, Some("lossless")), Some(preferred.clone()));
        fs::remove_file(preferred).unwrap();
        assert_eq!(find_local_song_file(&dir, 42, Some("lossless")), Some(fallback.clone()));
        // 兼容旧命名 {id}-{quality}-{name}
        fs::remove_file(fallback).unwrap();
        let legacy = dir.join("42-jymaster-song.flac");
        File::create(&legacy).unwrap();
        assert_eq!(find_local_song_file(&dir, 42, Some("jymaster")), Some(legacy));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn init_persists_default_download_path_for_old_settings() {
        let dir = test_dir();
        let settings_path = dir.join("settings.json");
        let downloads = dir.join("downloads");
        fs::write(&settings_path, r#"{"use_remote_api":false,"remote_api_url":"https://example.com/"}"#).unwrap();
        let mut client = NcmClient::new(PathBuf::new(), PathBuf::new(), PathBuf::new(), PathBuf::new(), settings_path.clone(), downloads.clone());

        client.init();

        let saved: Settings = serde_json::from_str(&fs::read_to_string(settings_path).unwrap()).unwrap();
        assert_eq!(saved.download_path, downloads);
        assert_eq!(saved.download_lyric_name_pattern, "{name}-Lyric");
        assert!(saved.download_path.is_dir());
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn skips_existing_song_with_same_quality() {
        let dir = test_dir();
        let existing = dir.join("song-a-x-lossless-42.flac");
        File::create(&existing).unwrap();
        let mut client = NcmClient::new(PathBuf::new(), PathBuf::new(), PathBuf::new(), PathBuf::new(), PathBuf::new(), dir.clone());
        client.settings.download_path = dir.clone();
        client.settings.download_quality = String::from("lossless");

        assert_eq!(client.download_song(song(42)).await.unwrap(), DownloadResult::AlreadyExists(existing));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn daily_window_starts_today_after_6am() {
        let now = Local.with_ymd_and_hms(2025, 1, 15, 6, 0, 0).unwrap();
        let window = daily_recommend_window_at(now);
        assert_eq!(window.len(), 14);
        assert_eq!(window[0], NaiveDate::from_ymd_opt(2025, 1, 15).unwrap());
        assert_eq!(window[13], NaiveDate::from_ymd_opt(2025, 1, 2).unwrap());
    }

    #[test]
    fn daily_window_shifts_back_before_6am() {
        let now = Local.with_ymd_and_hms(2025, 1, 15, 5, 59, 59).unwrap();
        let window = daily_recommend_window_at(now);
        assert_eq!(window.len(), 14);
        assert_eq!(window[0], NaiveDate::from_ymd_opt(2025, 1, 14).unwrap());
    }

    #[test]
    fn purge_removes_only_files_outside_window() {
        let dir = test_dir();
        let window = daily_recommend_window_at(Local.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap());
        File::create(dir.join("2025-01-15.json")).unwrap();
        File::create(dir.join("2025-01-01.json")).unwrap();
        File::create(dir.join("not-a-date.json")).unwrap();

        purge_daily_recommend_cache(&dir, &window);

        assert!(dir.join("2025-01-15.json").exists());
        assert!(!dir.join("2025-01-01.json").exists());
        assert!(dir.join("not-a-date.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
