use crate::config::{Command, ScreenEnum};
use crate::ui::panel::{PanelFocusedStatus, PlaylistPanel, SonglistsPanel};
use crate::ui::Controller;
use crate::{command_queue, ncm_client, player};
use log::debug;
use ncm_api::model::{Song, Songlist};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Style;
use ratatui::Frame;

#[derive(PartialEq)]
enum Panels {
    SonglistCandidates,
    SonglistContent,
}

#[derive(PartialEq)]
enum FocusPanel {
    SonglistCandidatesOutside,
    SonglistCandidatesInside,
    SonglistContentOutside,
    SonglistContentInside,
}

pub struct SonglistsScreen<'a> {
    current_focus_panel: FocusPanel,
    //
    current_selected_songlist: Option<Songlist>,
    //
    songlist_candidates_panel: SonglistsPanel<'a>,
    songlist_content_panel: PlaylistPanel<'a>,
}

impl<'a> SonglistsScreen<'a> {
    pub fn new(_normal_style: &Style) -> Self {
        Self {
            current_focus_panel: FocusPanel::SonglistCandidatesOutside,
            current_selected_songlist: None,
            songlist_candidates_panel: SonglistsPanel::new(PanelFocusedStatus::Outside),
            songlist_content_panel: PlaylistPanel::new(PanelFocusedStatus::Nop),
        }
    }

    pub fn selected_song(&self) -> Option<Song> {
        self.songlist_content_panel.get_selected_song()
    }

    pub fn selected_songlist(&self) -> Option<Songlist> {
        self.songlist_candidates_panel.get_selected_songlist()
    }
}

impl<'a> Controller for SonglistsScreen<'a> {
    async fn update_model(&mut self) -> anyhow::Result<bool> {
        let mut result = Ok(false);

        // songlist candidates
        if self.songlist_candidates_panel.update_model().await? {
            result = Ok(true);
        }

        // songlist content
        if self.songlist_content_panel.update_model().await? {
            result = Ok(true);
        }

        result
    }

    async fn handle_event(&mut self, cmd: Command) -> anyhow::Result<bool> {
        use Command::*;
        use FocusPanel::*;

        match (cmd.clone(), &self.current_focus_panel) {
            //
            (Esc, SonglistCandidatesInside) => {
                self.focus_panel_outside(Panels::SonglistCandidates);
            },
            (Esc, SonglistContentInside) => {
                self.focus_panel_outside(Panels::SonglistContent);
            },

            //
            (Down | Up, SonglistCandidatesOutside) => {
                self.focus_panel_inside(Panels::SonglistCandidates);
            },
            (Down | Up, SonglistContentOutside) => {
                self.focus_panel_inside(Panels::SonglistContent);
            },
            (Down | Up, SonglistCandidatesInside) => {
                self.songlist_candidates_panel.handle_event(cmd).await?;
            },
            (Down | Up, SonglistContentInside) => {
                self.songlist_content_panel.handle_event(cmd).await?;
            },

            //
            (NextPanel, SonglistCandidatesOutside) => {
                self.focus_panel_outside(Panels::SonglistContent);
            },
            (NextPanel, SonglistCandidatesInside) => {
                self.focus_panel_inside(Panels::SonglistContent);
            },
            (PrevPanel, SonglistContentOutside) => {
                self.focus_panel_outside(Panels::SonglistCandidates);
            },
            (PrevPanel, SonglistContentInside) => {
                self.focus_panel_inside(Panels::SonglistCandidates);
            },

            //
            (EnterOrPlay, SonglistCandidatesOutside) => {
                self.focus_panel_inside(Panels::SonglistCandidates);
            },
            (EnterOrPlay, SonglistContentOutside) => {
                self.focus_panel_inside(Panels::SonglistContent);
            },
            (EnterOrPlay, SonglistCandidatesInside) => {
                // 加载歌单
                if let Some(mut selected_songlist) = self.songlist_candidates_panel.get_selected_songlist() {
                    let ncm_client_guard = ncm_client.lock().await;
                    ncm_client_guard.load_songlist_songs(&mut selected_songlist).await?;
                    let downloaded_song_ids = ncm_client_guard.downloaded_song_ids();
                    drop(ncm_client_guard);

                    self.songlist_content_panel.set_model(&selected_songlist.name, &selected_songlist.songs, &downloaded_song_ids);

                    self.current_selected_songlist = Some(selected_songlist);
                }
            },
            // 切换歌单并从选中歌曲开始播放
            (EnterOrPlay | Play, SonglistContentInside) => {
                if let Some(selected_songlist_index) = self.songlist_candidates_panel.get_selected_songlist_index() {
                    debug!("切换到 {} 号歌单", selected_songlist_index);

                    // 切换当前播放列表
                    player.lock().await.switch_playlist(selected_songlist_index, ncm_client.lock().await).await?;

                    // 播放选中歌曲
                    self.songlist_content_panel.handle_event(cmd).await?;

                    // 返回 main_screen ，刷新播放列表显示
                    let mut command_queue_guard = command_queue.lock().await;
                    command_queue_guard.push_back(GotoScreen(ScreenEnum::Main));
                    command_queue_guard.push_back(RefreshPlaylist);
                    command_queue_guard.push_back(WhereIsThisSong);
                    drop(command_queue_guard);
                }
            },
            // 切换歌单并开始播放
            (Play, SonglistCandidatesInside) => {
                if let Some(selected_songlist_index) = self.songlist_candidates_panel.get_selected_songlist_index() {
                    debug!("切换到 {} 号歌单", selected_songlist_index);

                    // 切换当前播放列表
                    player.lock().await.switch_playlist(selected_songlist_index, ncm_client.lock().await).await?;

                    // 开始自动播放，返回 main_screen ，刷新播放列表显示
                    let mut command_queue_guard = command_queue.lock().await;
                    command_queue_guard.push_back(StartPlay);
                    command_queue_guard.push_back(GotoScreen(ScreenEnum::Main));
                    command_queue_guard.push_back(RefreshPlaylist);
                    command_queue_guard.push_back(WhereIsThisSong);
                    drop(command_queue_guard);
                }
            },

            //
            (GoToTop | GoToBottom, SonglistCandidatesOutside | SonglistCandidatesInside) => {
                self.songlist_candidates_panel.handle_event(cmd).await?;
            },
            (GoToTop | GoToBottom, SonglistContentOutside | SonglistContentInside) => {
                self.songlist_content_panel.handle_event(cmd).await?;
                self.focus_panel_inside(Panels::SonglistContent);
            },

            //
            (SearchForward(_) | SearchBackward(_), SonglistCandidatesOutside | SonglistCandidatesInside) => {
                self.songlist_candidates_panel.handle_event(cmd).await?;
            },
            (SearchForward(_) | SearchBackward(_), SonglistContentOutside | SonglistContentInside) => {
                self.songlist_content_panel.handle_event(cmd).await?;
                self.focus_panel_inside(Panels::SonglistContent);
            },

            // 删除选中的自建歌单（进入 App 确认流）
            (Delete, SonglistCandidatesInside) => {
                if let Some(songlist) = self.songlist_candidates_panel.get_selected_songlist() {
                    let mut command_queue_guard = command_queue.lock().await;
                    if songlist.subscribed {
                        command_queue_guard.push_back(ShowMessage("不能删除收藏的歌单".to_string()));
                    } else if songlist.special_type == 5 {
                        command_queue_guard.push_back(ShowMessage("不能删除「我喜欢的音乐」".to_string()));
                    } else {
                        command_queue_guard.push_back(DeleteSonglistByName(songlist.name));
                    }
                }
            },
            // 从当前浏览的歌单移除光标所在歌曲（进入 App 确认流）
            (Delete, SonglistContentInside) => {
                if let Some(songlist) = self.current_selected_songlist.clone() {
                    if songlist.subscribed || songlist.special_type == 5 {
                        command_queue.lock().await.push_back(ShowMessage("只能对自建歌单移除歌曲".to_string()));
                    } else if let Some(song) = self.songlist_content_panel.get_selected_song() {
                        command_queue.lock().await.push_back(RemoveSongFromSonglist {
                            songlist_id: songlist.id,
                            songlist_name: songlist.name,
                            song_id: song.id,
                            song_name: song.name,
                        });
                    }
                }
            },
            // 移除歌曲已远程成功，本地同步界面
            (SongRemovalDone { songlist_id, song_id }, _) => {
                if let Some(songlist) = self.current_selected_songlist.clone() {
                    if songlist.id == songlist_id {
                        let song_name = songlist.songs.iter().find(|s| s.id == song_id).map(|s| s.name.clone()).unwrap_or_default();
                        // 本地移除该歌曲并刷新右侧面板，避免服务端缓存导致的刷新滞后
                        let mut songlist = songlist;
                        songlist.songs.retain(|s| s.id != song_id);
                        songlist.songs_count = songlist.songs_count.saturating_sub(1);
                        let downloaded_song_ids = ncm_client.lock().await.downloaded_song_ids();
                        self.songlist_content_panel.set_model(&songlist.name, &songlist.songs, &downloaded_song_ids);
                        self.current_selected_songlist = Some(songlist);

                        // 同步左侧歌单计数
                        let mut player_guard = player.lock().await;
                        if let Some(sl) = player_guard.songlists_mut().iter_mut().find(|sl| sl.id == songlist_id) {
                            sl.songs_count = sl.songs_count.saturating_sub(1);
                        }
                        drop(player_guard);
                        command_queue.lock().await.push_back(RefreshPlaylist);
                        command_queue.lock().await.push_back(ShowMessage(format!("已从歌单移除：《{}》", song_name)));
                    }
                }
            },
            // 刷新左侧歌单列表
            (RefreshPlaylist, _) => {
                self.songlist_candidates_panel.handle_event(cmd).await?;
            },

            //
            (_, _) => {
                return Ok(false);
            },
        }

        Ok(true)
    }

    fn update_view(&mut self, style: &Style) {
        self.songlist_candidates_panel.update_view(style);

        self.songlist_content_panel.update_view(style);
    }

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        // 分为左右两个面板
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
            .split(chunk);

        // 在左半屏渲染 songlist_candidates_panel
        self.songlist_candidates_panel.draw(frame, chunks[0]);

        // 在右半屏渲染 songlist_content_panel
        self.songlist_content_panel.draw(frame, chunks[1]);
    }
}

/// private
impl<'a> SonglistsScreen<'a> {
    fn focus_panel_outside(&mut self, to_panel: Panels) {
        match to_panel {
            Panels::SonglistCandidates => {
                self.current_focus_panel = FocusPanel::SonglistCandidatesOutside;
                self.songlist_candidates_panel.focused_status = PanelFocusedStatus::Outside;
                self.songlist_content_panel.focused_status = PanelFocusedStatus::Nop;
            },
            Panels::SonglistContent => {
                self.current_focus_panel = FocusPanel::SonglistContentOutside;
                self.songlist_candidates_panel.focused_status = PanelFocusedStatus::Nop;
                self.songlist_content_panel.focused_status = PanelFocusedStatus::Outside;
            },
        }
    }

    fn focus_panel_inside(&mut self, to_panel: Panels) {
        match to_panel {
            Panels::SonglistCandidates => {
                self.current_focus_panel = FocusPanel::SonglistCandidatesInside;
                self.songlist_candidates_panel.focused_status = PanelFocusedStatus::Inside;
                self.songlist_content_panel.focused_status = PanelFocusedStatus::Nop;
            },
            Panels::SonglistContent => {
                self.current_focus_panel = FocusPanel::SonglistContentInside;
                self.songlist_candidates_panel.focused_status = PanelFocusedStatus::Nop;
                self.songlist_content_panel.focused_status = PanelFocusedStatus::Inside;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn switches_panels_while_inside() {
        let mut screen = SonglistsScreen::new(&Style::default());
        screen.focus_panel_inside(Panels::SonglistCandidates);

        screen.handle_event(Command::NextPanel).await.unwrap();
        assert!(screen.current_focus_panel == FocusPanel::SonglistContentInside);
        assert!(screen.songlist_content_panel.focused_status == PanelFocusedStatus::Inside);

        screen.handle_event(Command::PrevPanel).await.unwrap();
        assert!(screen.current_focus_panel == FocusPanel::SonglistCandidatesInside);
        assert!(screen.songlist_candidates_panel.focused_status == PanelFocusedStatus::Inside);
    }
}
