use crate::config::Command::SwitchPlayMode;
use crate::config::ScreenEnum;
use anyhow::{anyhow, Result};
use ncm_play::PlayMode;

#[derive(Clone, Debug)]
pub enum Command {
    Quit,
    GotoScreen(ScreenEnum),
    EnterCommand,
    Logout,
    PlayOrPause,
    SetVolume(f64),
    VolumeUp,
    VolumeDown,
    SwitchPlayMode(PlayMode),
    StartPlay,
    NextSong,
    PrevSong,
    /// 喜欢/取消喜欢光标所在歌曲，None 为切换
    SetSongLiked(Option<bool>),
    SearchForward(Vec<String>),
    SearchBackward(Vec<String>),
    RefreshPlaylist,
    DownloadSong,
    DownloadPlaylist,
    DownloadFinished(String),
    /// 在命令行区域显示一条提示消息
    ShowMessage(String),

    Down,
    Up,
    NextPanel,
    PrevPanel,
    Esc,
    /// Enter，优先执行进入某菜单的功能，无可进入（所选项为单曲）时播放
    EnterOrPlay,
    /// Alt + Enter，优先执行播放功能，所选项为菜单则对其执行 StartPlay
    Play,
    /// 收藏当前播放歌曲到指定歌单（greedy 参数：命令词后所有文本即歌单名）
    CollectToSonglist(String),
    /// 从指定歌单移除当前播放歌曲
    UncollectFromSonglist(String),
    /// 创建歌单（命令路径）
    CreateSonglist(String),
    /// 按名删除歌单（命令路径，需确认）
    DeleteSonglistByName(String),
    /// 歌单屏切换左列表「我创建的/我收藏的」视图（仅 c 键路径，无 :命令）
    ToggleSonglistView,
    /// 歌单屏置顶/取消置顶高亮歌单（仅 p 键路径，无 :命令）
    TogglePinSonglist,
    /// 置顶歌单上移/下移（Shift+K/↑、Shift+J/↓，仅快捷键路径）
    MovePinnedSonglistUp,
    MovePinnedSonglistDown,
    /// n 键（快捷键路径，按屏幕分派：主界面=收藏当前歌曲入口，歌单屏=新建歌单入口）
    NewOrCollect,
    /// 删除/移除（快捷键路径，由歌单屏按焦点解释：左侧=删歌单，右侧=移除光标歌曲）
    Delete,
    /// 请求从歌单移除歌曲（App 二次确认后执行）
    RemoveSongFromSonglist { songlist_id: u64, songlist_name: String, song_id: u64, song_name: String },
    /// 移除歌曲已确认并远程成功，通知界面执行本地移除
    SongRemovalDone { songlist_id: u64, song_id: u64 },
    /// 从当前播放列表移除光标歌曲（需二次确认）
    RemoveFromCurrentPlaylist,
    WhereIsThisSong,
    /// 播放自动切歌后同步 playlist 光标（不改变面板焦点）
    SyncPlaylistCursor,
    GoToTop,
    GoToBottom,

    Nop,
}

impl Command {
    pub fn parse(cmd_str: &str) -> Result<Self> {
        let mut tokens = cmd_str.split_whitespace();

        match tokens.next() {
            Some("q" | "quit" | "exit") => Ok(Self::Quit),
            Some("screen") => match tokens.next() {
                Some("1" | "main") => Ok(Self::GotoScreen(ScreenEnum::Main)),
                Some("2" | "playlist" | "playlists") => Ok(Self::GotoScreen(ScreenEnum::Songlists)),
                Some("3") => Ok(Self::GotoScreen(ScreenEnum::Daily)),
                Some("9" | "settings") => Ok(Self::GotoScreen(ScreenEnum::Settings)),
                Some("0" | "help") => Ok(Self::GotoScreen(ScreenEnum::Help)),
                Some(other) => Err(anyhow!("screen: Invalid screen identifier: {}", other)),
                None => Err(anyhow!("screen: Missing argument SCREEN_ID")),
            },
            Some("h" | "help") => Ok(Self::GotoScreen(ScreenEnum::Help)),
            Some("l" | "login") => Ok(Self::GotoScreen(ScreenEnum::Login)),
            Some("logout") => Ok(Self::Logout),
            Some("vol" | "volume") => match tokens.next() {
                Some(num) => {
                    if let Ok(vol) = num.parse::<f64>() {
                        Ok(Self::SetVolume(vol / 100.0))
                    } else {
                        Err(anyhow!("volume: Invalid argument NUMBER"))
                    }
                },
                None => Err(anyhow!("volume: Missing argument NUMBER")),
            },
            Some("mute") => Ok(Self::SetVolume(0.0)),
            Some("mode") => match tokens.next() {
                Some("single") => Ok(SwitchPlayMode(PlayMode::Single)),
                Some("sr" | "single-repeat") => Ok(SwitchPlayMode(PlayMode::SingleRepeat)),
                Some("lr" | "list-repeat") => Ok(SwitchPlayMode(PlayMode::ListRepeat)),
                Some("s" | "shuf" | "shuffle") => Ok(SwitchPlayMode(PlayMode::Shuffle)),
                Some(other) => Err(anyhow!("switch: Invalid play mode identifier: {}", other)),
                None => Err(anyhow!("switch: Missing argument PLAY_MODE")),
            },
            Some("next") => Ok(Self::NextSong),
            Some("prev" | "previous") => Ok(Self::PrevSong),
            Some("like") => Ok(Self::SetSongLiked(Some(true))),
            Some("unlike") => Ok(Self::SetSongLiked(Some(false))),
            Some("start") => Ok(Self::StartPlay),
            Some("remove") => match tokens.next() {
                None => Ok(Self::RemoveFromCurrentPlaylist),
                Some(_) => Err(anyhow!("remove: Too many arguments")),
            },
            Some("download") => match (tokens.next(), tokens.next()) {
                (Some("song"), None) => Ok(Self::DownloadSong),
                (Some("playlist"), None) => Ok(Self::DownloadPlaylist),
                (None, _) => Err(anyhow!("download: Missing argument song|playlist")),
                (Some(other), None) => Err(anyhow!("download: Invalid target '{}'", other)),
                (_, Some(_)) => Err(anyhow!("download: Too many arguments")),
            },
            Some("collect") => {
                let name = cmd_str.strip_prefix("collect").unwrap().trim();
                if name.is_empty() {
                    Err(anyhow!("collect: Missing argument SONGLIST_NAME"))
                } else {
                    Ok(Self::CollectToSonglist(name.to_string()))
                }
            },
            Some("uncollect") => {
                let name = cmd_str.strip_prefix("uncollect").unwrap().trim();
                if name.is_empty() {
                    Err(anyhow!("uncollect: Missing argument SONGLIST_NAME"))
                } else {
                    Ok(Self::UncollectFromSonglist(name.to_string()))
                }
            },
            Some("playlist") => {
                let rest = cmd_str.strip_prefix("playlist").unwrap().trim();
                match tokens.next() {
                    Some("create") => {
                        let name = rest.strip_prefix("create").unwrap().trim();
                        if name.is_empty() {
                            Err(anyhow!("playlist create: Missing argument SONGLIST_NAME"))
                        } else {
                            Ok(Self::CreateSonglist(name.to_string()))
                        }
                    },
                    Some("delete") => {
                        let name = rest.strip_prefix("delete").unwrap().trim();
                        if name.is_empty() {
                            Err(anyhow!("playlist delete: Missing argument SONGLIST_NAME"))
                        } else {
                            Ok(Self::DeleteSonglistByName(name.to_string()))
                        }
                    },
                    Some(other) => Err(anyhow!("playlist: Invalid action '{}'", other)),
                    None => Err(anyhow!("playlist: Missing argument create|delete")),
                }
            },
            Some("where") => match tokens.next() {
                Some("this") => Ok(Self::WhereIsThisSong),
                Some(other) => Err(anyhow!("where: Invalid argument '{}'", other)),
                None => Err(anyhow!("where: Missing argument")),
            },
            Some("top") => Ok(Self::GoToTop),
            Some("bottom") => Ok(Self::GoToBottom),
            Some("/") => {
                let mut keywords = Vec::new();
                while let Some(keyword) = tokens.next() {
                    keywords.push(keyword.to_string());
                }
                Ok(Self::SearchForward(keywords))
            },
            Some("?") => {
                let mut keywords = Vec::new();
                while let Some(keyword) = tokens.next() {
                    keywords.push(keyword.to_string());
                }
                Ok(Self::SearchBackward(keywords))
            },
            Some(other) => Err(anyhow!("Invalid command: {}", other)),
            None => Ok(Self::Nop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Command;
    use crate::config::ScreenEnum;

    #[test]
    fn parses_screen_3() {
        assert!(matches!(Command::parse("screen 3").unwrap(), Command::GotoScreen(ScreenEnum::Daily)));
    }

    #[test]
    fn parses_screen_9() {
        assert!(matches!(Command::parse("screen 9").unwrap(), Command::GotoScreen(ScreenEnum::Settings)));
        assert!(matches!(Command::parse("screen settings").unwrap(), Command::GotoScreen(ScreenEnum::Settings)));
    }

    #[test]
    fn parses_like_commands() {
        assert!(matches!(
            Command::parse("like").unwrap(),
            Command::SetSongLiked(Some(true))
        ));
        assert!(matches!(
            Command::parse("unlike").unwrap(),
            Command::SetSongLiked(Some(false))
        ));
    }

    #[test]
    fn parses_download_commands_strictly() {
        assert!(matches!(Command::parse("download song").unwrap(), Command::DownloadSong));
        assert!(matches!(Command::parse("download playlist").unwrap(), Command::DownloadPlaylist));
        assert!(Command::parse("download").is_err());
        assert!(Command::parse("download album").is_err());
        assert!(Command::parse("download song extra").is_err());
    }

    #[test]
    fn parses_collect_commands_greedily() {
        assert!(matches!(
            Command::parse("collect 我的 跑步 歌单").unwrap(),
            Command::CollectToSonglist(name) if name == "我的 跑步 歌单"
        ));
        assert!(matches!(
            Command::parse("uncollect 跑步").unwrap(),
            Command::UncollectFromSonglist(name) if name == "跑步"
        ));
        assert!(Command::parse("collect").is_err());
        assert!(Command::parse("uncollect").is_err());
    }

    #[test]
    fn parses_playlist_commands_greedily() {
        assert!(matches!(
            Command::parse("playlist create 新 歌单").unwrap(),
            Command::CreateSonglist(name) if name == "新 歌单"
        ));
        assert!(matches!(
            Command::parse("playlist delete 旧歌单").unwrap(),
            Command::DeleteSonglistByName(name) if name == "旧歌单"
        ));
        assert!(Command::parse("playlist").is_err());
        assert!(Command::parse("playlist create").is_err());
        assert!(Command::parse("playlist rename x").is_err());
    }

    #[test]
    fn parses_remove_command_strictly() {
        assert!(matches!(Command::parse("remove").unwrap(), Command::RemoveFromCurrentPlaylist));
        assert!(Command::parse("remove extra").is_err());
    }
}
