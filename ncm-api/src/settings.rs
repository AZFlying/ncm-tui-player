use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DOWNLOAD_QUALITIES: [&str; 9] = [
    "standard", "higher", "exhigh", "lossless", "hires", "jyeffect", "sky", "dolby", "jymaster",
];

/// 音质英文 key → 网易云官方中文名
pub const QUALITY_DISPLAY_NAMES: [(&str, &str); 9] = [
    ("standard", "标准"),
    ("higher", "较高"),
    ("exhigh", "极高"),
    ("lossless", "无损"),
    ("hires", "Hi-Res"),
    ("jyeffect", "高清臻音"),
    ("sky", "沉浸环绕声"),
    ("dolby", "杜比全景声"),
    ("jymaster", "超清母带"),
];

/// 音质的显示名（未知 key 原样返回）
pub fn quality_display_name(key: &str) -> &str {
    QUALITY_DISPLAY_NAMES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, name)| *name)
        .unwrap_or(key)
}

/// 在音质档位中循环（forward=true 向右），未知 key 回到第一档
pub fn cycle_quality(key: &str, forward: bool) -> &'static str {
    let len = DOWNLOAD_QUALITIES.len();
    let index = DOWNLOAD_QUALITIES.iter().position(|q| *q == key).unwrap_or(0);
    let next = if forward { (index + 1) % len } else { (index + len - 1) % len };
    DOWNLOAD_QUALITIES[next]
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub use_remote_api: bool,
    pub remote_api_url: String,
    pub download_path: PathBuf,
    pub download_quality: String,
    pub play_quality: String,
    pub download_file_name_pattern: String,
    pub download_lyric_name_pattern: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            use_remote_api: false,
            remote_api_url: String::from("https://ncm-api-wine.vercel.app/"),
            download_path: PathBuf::new(),
            download_quality: String::from("jymaster"),
            play_quality: String::from("hires"),
            download_file_name_pattern: String::from("{name}-{singer}-{album}-{quality}-{id}"),
            download_lyric_name_pattern: String::from("{name}-Lyric"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cycle_quality, quality_display_name, Settings};

    #[test]
    fn old_settings_use_download_defaults() {
        let settings: Settings = serde_json::from_str(
            r#"{"use_remote_api":true,"remote_api_url":"https://example.com/"}"#,
        )
        .unwrap();

        assert!(settings.download_path.as_os_str().is_empty());
        assert_eq!(settings.download_quality, "jymaster");
        assert_eq!(settings.play_quality, "hires");
        assert_eq!(settings.download_file_name_pattern, "{name}-{singer}-{album}-{quality}-{id}");
        assert_eq!(settings.download_lyric_name_pattern, "{name}-Lyric");
    }

    #[test]
    fn maps_quality_display_names() {
        assert_eq!(quality_display_name("standard"), "标准");
        assert_eq!(quality_display_name("hires"), "Hi-Res");
        assert_eq!(quality_display_name("jymaster"), "超清母带");
        assert_eq!(quality_display_name("unknown"), "unknown");
    }

    #[test]
    fn cycles_quality_wrapping_around() {
        assert_eq!(cycle_quality("standard", true), "higher");
        assert_eq!(cycle_quality("jymaster", true), "standard");
        assert_eq!(cycle_quality("standard", false), "jymaster");
        assert_eq!(cycle_quality("hires", false), "lossless");
        assert_eq!(cycle_quality("unknown", true), "higher");
    }
}
