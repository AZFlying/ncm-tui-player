use crate::config::{Command, ScreenEnum};
use crate::ui::panel::{daily_list_label, DailyListsPanel, PanelFocusedStatus, PlaylistPanel};
use crate::ui::Controller;
use crate::{command_queue, ncm_client, player};
use log::debug;
use ncm_api::model::{Song, Songlist};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Style;
use ratatui::Frame;

#[derive(PartialEq)]
enum Panels {
    DailyLists,
    DailyContent,
}

#[derive(PartialEq)]
enum FocusPanel {
    DailyListsOutside,
    DailyListsInside,
    DailyContentOutside,
    DailyContentInside,
}

pub struct DailyScreen<'a> {
    current_focus_panel: FocusPanel,
    //
    current_selected_songlist: Option<Songlist>,
    //
    daily_lists_panel: DailyListsPanel<'a>,
    daily_content_panel: PlaylistPanel<'a>,
}

impl<'a> DailyScreen<'a> {
    pub fn new(_normal_style: &Style) -> Self {
        Self {
            current_focus_panel: FocusPanel::DailyListsOutside,
            current_selected_songlist: None,
            daily_lists_panel: DailyListsPanel::new(PanelFocusedStatus::Outside),
            daily_content_panel: PlaylistPanel::new(PanelFocusedStatus::Nop),
        }
    }

    pub fn selected_song(&self) -> Option<Song> {
        self.daily_content_panel.get_selected_song()
    }

    pub fn selected_songlist(&self) -> Option<Songlist> {
        self.current_selected_songlist.clone()
    }
}

impl<'a> Controller for DailyScreen<'a> {
    async fn update_model(&mut self) -> anyhow::Result<bool> {
        let mut result = Ok(false);

        // daily lists
        if self.daily_lists_panel.update_model().await? {
            result = Ok(true);
        }

        // daily content
        if self.daily_content_panel.update_model().await? {
            result = Ok(true);
        }

        result
    }

    async fn handle_event(&mut self, cmd: Command) -> anyhow::Result<bool> {
        use Command::*;
        use FocusPanel::*;

        match (cmd.clone(), &self.current_focus_panel) {
            //
            (Esc, DailyListsInside) => {
                self.focus_panel_outside(Panels::DailyLists);
            },
            (Esc, DailyContentInside) => {
                self.focus_panel_outside(Panels::DailyContent);
            },

            //
            (Down | Up, DailyListsOutside) => {
                self.focus_panel_inside(Panels::DailyLists);
            },
            (Down | Up, DailyContentOutside) => {
                self.focus_panel_inside(Panels::DailyContent);
            },
            (Down | Up, DailyListsInside) => {
                self.daily_lists_panel.handle_event(cmd).await?;
            },
            (Down | Up, DailyContentInside) => {
                self.daily_content_panel.handle_event(cmd).await?;
            },

            //
            (NextPanel, DailyListsOutside) => {
                self.focus_panel_outside(Panels::DailyContent);
            },
            (NextPanel, DailyListsInside) => {
                self.focus_panel_inside(Panels::DailyContent);
            },
            (PrevPanel, DailyContentOutside) => {
                self.focus_panel_outside(Panels::DailyLists);
            },
            (PrevPanel, DailyContentInside) => {
                self.focus_panel_inside(Panels::DailyLists);
            },

            //
            (EnterOrPlay, DailyListsOutside) => {
                self.focus_panel_inside(Panels::DailyLists);
            },
            (EnterOrPlay, DailyContentOutside) => {
                self.focus_panel_inside(Panels::DailyContent);
            },
            (EnterOrPlay, DailyListsInside) => {
                // 加载选中日推
                if let Err(err) = self.load_selected_daily().await {
                    command_queue.lock().await.push_back(ShowMessage(err.to_string()));
                }
            },
            // 切换日推并从选中歌曲开始播放
            (EnterOrPlay | Play, DailyContentInside) => {
                if let Some(songlist) = self.current_selected_songlist.clone() {
                    debug!("切换到日推 {}", songlist.name);

                    // 切换当前播放列表
                    player.lock().await.switch_to_songlist(songlist);

                    // 播放选中歌曲
                    self.daily_content_panel.handle_event(cmd).await?;

                    // 返回 main_screen ，刷新播放列表显示
                    let mut command_queue_guard = command_queue.lock().await;
                    command_queue_guard.push_back(GotoScreen(ScreenEnum::Main));
                    command_queue_guard.push_back(RefreshPlaylist);
                    command_queue_guard.push_back(WhereIsThisSong);
                    drop(command_queue_guard);
                }
            },
            // 加载选中日推并开始播放
            (Play, DailyListsInside) => {
                if let Err(err) = self.load_selected_daily().await {
                    command_queue.lock().await.push_back(ShowMessage(err.to_string()));
                } else if let Some(songlist) = self.current_selected_songlist.clone() {
                    debug!("切换到日推 {}", songlist.name);

                    // 切换当前播放列表
                    player.lock().await.switch_to_songlist(songlist);

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
            (GoToTop | GoToBottom, DailyListsOutside | DailyListsInside) => {
                self.daily_lists_panel.handle_event(cmd).await?;
            },
            (GoToTop | GoToBottom, DailyContentOutside | DailyContentInside) => {
                self.daily_content_panel.handle_event(cmd).await?;
                self.focus_panel_inside(Panels::DailyContent);
            },

            //
            (SearchForward(_) | SearchBackward(_), DailyListsOutside | DailyListsInside) => {
                self.daily_lists_panel.handle_event(cmd).await?;
            },
            (SearchForward(_) | SearchBackward(_), DailyContentOutside | DailyContentInside) => {
                self.daily_content_panel.handle_event(cmd).await?;
                self.focus_panel_inside(Panels::DailyContent);
            },

            // 日推列表为服务端生成，不可编辑
            (Delete, _) => {
                command_queue.lock().await.push_back(ShowMessage("日推列表不支持删除".to_string()));
            },

            //
            (_, _) => {
                return Ok(false);
            },
        }

        Ok(true)
    }

    fn update_view(&mut self, style: &Style) {
        self.daily_lists_panel.update_view(style);

        self.daily_content_panel.update_view(style);
    }

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        // 分为左右两个面板
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
            .split(chunk);

        // 在左半屏渲染 daily_lists_panel
        self.daily_lists_panel.draw(frame, chunks[0]);

        // 在右半屏渲染 daily_content_panel
        self.daily_content_panel.draw(frame, chunks[1]);
    }
}

/// private
impl<'a> DailyScreen<'a> {
    /// 加载左栏选中日期的日推歌曲到右栏，并组装为伪歌单（id 占位为 0，不与服务端交互）
    async fn load_selected_daily(&mut self) -> anyhow::Result<()> {
        if let Some(date) = self.daily_lists_panel.get_selected_date() {
            let (songs, downloaded_song_ids) = {
                let ncm_client_guard = ncm_client.lock().await;
                let songs = ncm_client_guard.get_daily_recommend_songs(date).await?;
                let downloaded_song_ids = ncm_client_guard.downloaded_song_ids();
                (songs, downloaded_song_ids)
            };

            let name = daily_list_label(date);
            self.daily_content_panel.set_model(&name, &songs, &downloaded_song_ids);

            self.current_selected_songlist = Some(Songlist {
                name,
                id: 0,
                songs_count: songs.len(),
                creator: String::new(),
                subscribed: false,
                special_type: 0,
                songs,
            });
        }

        Ok(())
    }

    fn focus_panel_outside(&mut self, to_panel: Panels) {
        match to_panel {
            Panels::DailyLists => {
                self.current_focus_panel = FocusPanel::DailyListsOutside;
                self.daily_lists_panel.focused_status = PanelFocusedStatus::Outside;
                self.daily_content_panel.focused_status = PanelFocusedStatus::Nop;
            },
            Panels::DailyContent => {
                self.current_focus_panel = FocusPanel::DailyContentOutside;
                self.daily_lists_panel.focused_status = PanelFocusedStatus::Nop;
                self.daily_content_panel.focused_status = PanelFocusedStatus::Outside;
            },
        }
    }

    fn focus_panel_inside(&mut self, to_panel: Panels) {
        match to_panel {
            Panels::DailyLists => {
                self.current_focus_panel = FocusPanel::DailyListsInside;
                self.daily_lists_panel.focused_status = PanelFocusedStatus::Inside;
                self.daily_content_panel.focused_status = PanelFocusedStatus::Nop;
            },
            Panels::DailyContent => {
                self.current_focus_panel = FocusPanel::DailyContentInside;
                self.daily_lists_panel.focused_status = PanelFocusedStatus::Nop;
                self.daily_content_panel.focused_status = PanelFocusedStatus::Inside;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn switches_panels_while_inside() {
        let mut screen = DailyScreen::new(&Style::default());
        screen.focus_panel_inside(Panels::DailyLists);

        screen.handle_event(Command::NextPanel).await.unwrap();
        assert!(screen.current_focus_panel == FocusPanel::DailyContentInside);
        assert!(screen.daily_content_panel.focused_status == PanelFocusedStatus::Inside);

        screen.handle_event(Command::PrevPanel).await.unwrap();
        assert!(screen.current_focus_panel == FocusPanel::DailyListsInside);
        assert!(screen.daily_lists_panel.focused_status == PanelFocusedStatus::Inside);
    }
}
