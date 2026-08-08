use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DOWNLOAD_QUALITIES: [&str; 9] = [
    "standard", "higher", "exhigh", "lossless", "hires", "jyeffect", "sky", "dolby", "jymaster",
];

#[derive(Deserialize, Serialize, Debug, Clone)]
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
    use super::Settings;

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
}
