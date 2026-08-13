use crate::config::style::*;
use crate::config::Command;
use crate::ui::panel::{centered_offset, PanelFocusedStatus};
use crate::ui::Controller;
use crate::{ncm_client, player};
use ncm_api::model::Songlist;
use ratatui::layout::{Margin, Rect};
use ratatui::prelude::{Constraint, Style};
use ratatui::style::palette::tailwind;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState};
use ratatui::Frame;

#[derive(Clone, Copy, PartialEq)]
enum SonglistView {
    Created,
    Subscribed,
}

/// 按视图过滤歌单，返回 (全量列表原始索引, 歌单) 列表
fn filtered_with_indices(songlists: &[Songlist], view: SonglistView) -> Vec<(usize, &Songlist)> {
    songlists
        .iter()
        .enumerate()
        .filter(|(_, s)| s.subscribed == (view == SonglistView::Subscribed))
        .collect()
}

/// 置顶项按 pinned 顺序排在前，其余保持原顺序；pinned 中的失效 id 跳过
fn order_with_pins<'s>(filtered: Vec<(usize, &'s Songlist)>, pinned: &[u64]) -> Vec<(usize, &'s Songlist)> {
    let mut ordered = Vec::with_capacity(filtered.len());
    for id in pinned {
        if let Some(item) = filtered.iter().find(|(_, songlist)| songlist.id == *id) {
            ordered.push(*item);
        }
    }
    ordered.extend(filtered.into_iter().filter(|(_, songlist)| !pinned.contains(&songlist.id)));
    ordered
}

/// 存在则移除，否则追加到末尾
fn toggle_pin(pinned: &mut Vec<u64>, id: u64) {
    if let Some(pos) = pinned.iter().position(|pinned_id| *pinned_id == id) {
        pinned.remove(pos);
    } else {
        pinned.push(id);
    }
}

/// 与相邻置顶项交换；不在列表或越界返回 false
fn move_pin(pinned: &mut Vec<u64>, id: u64, up: bool) -> bool {
    if let Some(pos) = pinned.iter().position(|pinned_id| *pinned_id == id) {
        let target = if up { pos.checked_sub(1) } else { (pos + 1 < pinned.len()).then_some(pos + 1) };
        if let Some(target) = target {
            pinned.swap(pos, target);
            return true;
        }
    }
    false
}

pub struct SonglistsPanel<'a> {
    // model
    pub focused_status: PanelFocusedStatus, // 聚焦状态交给父 screen 管理，面板自身只读不写
    //
    username: String,
    view: SonglistView,
    songlists: Vec<Songlist>,
    original_indices: Vec<usize>, // 显示行 -> player.songlists() 全量索引
    pending_select_id: Option<u64>, // 置顶/排序重建后按 id 恢复选中，让光标跟随目标歌单
    songlists_table_rows: Vec<Row<'a>>,
    songlists_table_state: TableState,
    scrollbar_state: ScrollbarState,

    // view
    songlists_table: Table<'a>,
}

impl<'a> SonglistsPanel<'a> {
    pub fn new(focused_status: PanelFocusedStatus) -> Self {
        Self {
            focused_status,
            username: String::new(),
            view: SonglistView::Created,
            songlists: Vec::new(),
            original_indices: Vec::new(),
            pending_select_id: None,
            songlists_table_rows: Vec::new(),
            songlists_table_state: TableState::new(),
            scrollbar_state: ScrollbarState::new(0),
            songlists_table: Table::default(),
        }
    }
}

impl<'a> SonglistsPanel<'a> {
    pub fn get_selected_songlist(&self) -> Option<Songlist> {
        if let Some(selected) = self.songlists_table_state.selected() {
            if let Some(songlist) = self.songlists.get(selected) {
                return Some(songlist.clone());
            }
        }

        None
    }

    pub fn get_selected_songlist_index(&self) -> Option<usize> {
        // 返回全量列表索引，保证 switch_playlist 切对歌单
        self.songlists_table_state.selected().and_then(|selected| self.original_indices.get(selected).copied())
    }
}

impl<'a> Controller for SonglistsPanel<'a> {
    async fn update_model(&mut self) -> anyhow::Result<bool> {
        let mut result = Ok(false);

        if self.songlists_table_rows.is_empty() {
            let player_guard = player.lock().await;
            let user_all_songlists = player_guard.songlists();

            let filtered = filtered_with_indices(user_all_songlists, self.view);
            let ncm_client_guard = ncm_client.lock().await;
            if let Some(login_account) = ncm_client_guard.login_account() {
                self.username = login_account.nickname;
            }
            let pinned = match self.view {
                SonglistView::Created => ncm_client_guard.settings().pinned_created_songlists,
                SonglistView::Subscribed => ncm_client_guard.settings().pinned_subscribed_songlists,
            };
            drop(ncm_client_guard);

            let ordered = order_with_pins(filtered, &pinned);
            self.original_indices = ordered.iter().map(|(index, _)| *index).collect();
            self.songlists = ordered.iter().map(|(_, songlist)| (*songlist).clone()).collect();
            self.songlists_table_rows = ordered
                .iter()
                .map(|(_, songlist)| {
                    let name = if pinned.contains(&songlist.id) {
                        format!("↑ {}", songlist.name)
                    } else {
                        songlist.name.clone()
                    };
                    Row::from_iter(vec![
                        Cell::new(name),
                        Cell::new(songlist.creator.clone()),
                        Cell::new(format!("{:>6}", songlist.songs_count)),
                    ])
                })
                .collect();

            drop(player_guard);

            // 防止悬空
            self.songlists_table_state.select(None);

            self.scrollbar_state = ScrollbarState::new(self.songlists_table_rows.len());

            // 置顶/排序后光标跟随目标歌单
            if let Some(pending_id) = self.pending_select_id.take() {
                if let Some(pos) = self.songlists.iter().position(|songlist| songlist.id == pending_id) {
                    self.songlists_table_state.select(Some(pos));
                    self.scrollbar_state = self.scrollbar_state.position(pos);
                }
            }

            result = Ok(true);
        }

        if self.songlists_table_state.selected() == None && !self.songlists_table_rows.is_empty() {
            self.songlists_table_state.select(Some(0));
            self.scrollbar_state.first();
            result = Ok(true);
        }

        result
    }

    async fn handle_event(&mut self, cmd: Command) -> anyhow::Result<bool> {
        match cmd {
            Command::Down => {
                // 直接使用 select_next() 存在越界问题
                if let (Some(selected), list_len) = (self.songlists_table_state.selected(), self.songlists_table_rows.len()) {
                    if selected + 1 < list_len {
                        self.songlists_table_state.select_next();
                        self.scrollbar_state.next();
                    }
                }
            },
            Command::Up => {
                self.songlists_table_state.select_previous();
                self.scrollbar_state.prev();
            },
            Command::EnterOrPlay => {},
            Command::GoToTop => {
                self.songlists_table_state.select_first();
                self.scrollbar_state.first();
            },
            Command::GoToBottom => {
                // 使用 select_last() 会越界；收藏视图可能为空，防空列表下溢
                if !self.songlists_table_rows.is_empty() {
                    self.songlists_table_state.select(Some(self.songlists_table_rows.len() - 1));
                    self.scrollbar_state.last();
                }
            },
            Command::ToggleSonglistView => {
                self.view = match self.view {
                    SonglistView::Created => SonglistView::Subscribed,
                    SonglistView::Subscribed => SonglistView::Created,
                };
                // 清空缓存行，update_model 按新视图重建；选中悬空后重置到首行
                self.songlists_table_rows.clear();
                self.songlists_table_state.select(None);
            },
            Command::SearchForward(_) => {},
            Command::SearchBackward(_) => {},
            Command::TogglePinSonglist => {
                if let Some(songlist) = self.get_selected_songlist() {
                    let mut ncm_client_guard = ncm_client.lock().await;
                    let mut settings = ncm_client_guard.settings();
                    let pinned = match self.view {
                        SonglistView::Created => &mut settings.pinned_created_songlists,
                        SonglistView::Subscribed => &mut settings.pinned_subscribed_songlists,
                    };
                    toggle_pin(pinned, songlist.id);
                    ncm_client_guard.update_settings(settings);
                    drop(ncm_client_guard);

                    // 重建行，光标跟随目标歌单
                    self.pending_select_id = Some(songlist.id);
                    self.songlists_table_rows.clear();
                }
            },
            Command::MovePinnedSonglistUp | Command::MovePinnedSonglistDown => {
                if let Some(songlist) = self.get_selected_songlist() {
                    let mut ncm_client_guard = ncm_client.lock().await;
                    let mut settings = ncm_client_guard.settings();
                    let pinned = match self.view {
                        SonglistView::Created => &mut settings.pinned_created_songlists,
                        SonglistView::Subscribed => &mut settings.pinned_subscribed_songlists,
                    };
                    // 光标在非置顶歌单或边界处时无操作（不清缓存、不写盘）
                    if move_pin(pinned, songlist.id, matches!(cmd, Command::MovePinnedSonglistUp)) {
                        ncm_client_guard.update_settings(settings);
                        drop(ncm_client_guard);

                        self.pending_select_id = Some(songlist.id);
                        self.songlists_table_rows.clear();
                    }
                }
            },
            Command::RefreshPlaylist => {
                // 清空缓存行，update_model 时从 player 重新拉取
                self.songlists_table_rows.clear();
            },
            _ => {},
        }

        Ok(true)
    }

    fn update_view(&mut self, _style: &Style) {
        let mut songlists_table = Table::new(self.songlists_table_rows.clone(), [Constraint::Min(30), Constraint::Min(10), Constraint::Max(6)])
            .header(Row::new(vec![Cell::new("歌单"), Cell::new("创建者"), Cell::new("歌曲数")]).style(TABLE_HEADER_STYLE).height(1))
            .block({
                let mut block = Block::default()
                    .title(Line::from(format!(
                        "{}{}的歌单",
                        self.username,
                        match self.view {
                            SonglistView::Created => "创建",
                            SonglistView::Subscribed => "收藏",
                        }
                    )))
                    .title(
                        Line::from(vec![
                            Span::styled("c", Style::default().fg(tailwind::RED.c400).add_modifier(Modifier::BOLD)),
                            Span::raw("hange"),
                        ])
                        .right_aligned(),
                    )
                    .title_bottom(Line::from("按下`Alt+Enter`开始播放选中歌单").centered())
                    .borders(Borders::ALL);
                if self.focused_status != PanelFocusedStatus::Nop {
                    block = block.border_style(PANEL_SELECTED_BORDER_STYLE);
                }

                block
            });

        // highlight
        if self.focused_status == PanelFocusedStatus::Inside {
            songlists_table = songlists_table.row_highlight_style(ITEM_SELECTED_STYLE).highlight_symbol(">")
        }

        self.songlists_table = songlists_table;
    }

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        let mut songlists_table_state = self.songlists_table_state.clone();
        if let Some(selected) = songlists_table_state.selected() {
            let visible_rows = chunk.height.saturating_sub(3) as usize; // 上下边框和表头
            *songlists_table_state.offset_mut() =
                centered_offset(selected, self.songlists_table_rows.len(), visible_rows);
        }
        frame.render_stateful_widget(&self.songlists_table, chunk, &mut songlists_table_state);

        // 渲染 scrollbar
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .track_symbol(None)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(tailwind::ROSE.c800);
        let scrollbar_area = chunk.inner(Margin { vertical: 1, horizontal: 0 });
        let mut scrollbar_state = self.scrollbar_state.clone();
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songlist(id: u64, subscribed: bool) -> Songlist {
        Songlist {
            name: format!("songlist{}", id),
            id,
            songs_count: 0,
            creator: String::new(),
            subscribed,
            special_type: 0,
            songs: Vec::new(),
        }
    }

    #[test]
    fn filters_by_view_and_keeps_original_indices() {
        let songlists = vec![songlist(1, false), songlist(2, true), songlist(3, false), songlist(4, true)];

        let created = filtered_with_indices(&songlists, SonglistView::Created);
        assert_eq!(created.iter().map(|(i, s)| (*i, s.id)).collect::<Vec<_>>(), vec![(0, 1), (2, 3)]);

        let subscribed = filtered_with_indices(&songlists, SonglistView::Subscribed);
        assert_eq!(subscribed.iter().map(|(i, s)| (*i, s.id)).collect::<Vec<_>>(), vec![(1, 2), (3, 4)]);
    }

    #[test]
    fn orders_pinned_first_in_pin_order_and_skips_stale_ids() {
        let songlists = vec![songlist(1, false), songlist(2, false), songlist(3, false), songlist(4, false)];
        let filtered = filtered_with_indices(&songlists, SonglistView::Created);

        let ordered = order_with_pins(filtered, &[3, 1, 999]);
        assert_eq!(ordered.iter().map(|(_, s)| s.id).collect::<Vec<_>>(), vec![3, 1, 2, 4]);
        // 原始索引随排序一起走
        assert_eq!(ordered.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![2, 0, 1, 3]);
    }

    #[test]
    fn toggle_pin_adds_and_removes() {
        let mut pinned = vec![];
        toggle_pin(&mut pinned, 1);
        toggle_pin(&mut pinned, 2);
        assert_eq!(pinned, vec![1, 2]);
        toggle_pin(&mut pinned, 1);
        assert_eq!(pinned, vec![2]);
    }

    #[test]
    fn move_pin_swaps_and_rejects_boundaries() {
        let mut pinned = vec![1, 2, 3];
        assert!(move_pin(&mut pinned, 2, true));
        assert_eq!(pinned, vec![2, 1, 3]);
        assert!(move_pin(&mut pinned, 2, false));
        assert_eq!(pinned, vec![1, 2, 3]);
        assert!(!move_pin(&mut pinned, 1, true)); // 顶部越界
        assert!(!move_pin(&mut pinned, 3, false)); // 底部越界
        assert!(!move_pin(&mut pinned, 999, true)); // 非成员
    }

    #[tokio::test]
    async fn toggle_switches_view_and_clears_cache() {
        let mut panel = SonglistsPanel::new(PanelFocusedStatus::Nop);
        panel.songlists_table_rows.push(Row::new(vec![Cell::new("x")]));

        panel.handle_event(Command::ToggleSonglistView).await.unwrap();
        assert!(panel.view == SonglistView::Subscribed);
        assert!(panel.songlists_table_rows.is_empty());

        panel.handle_event(Command::ToggleSonglistView).await.unwrap();
        assert!(panel.view == SonglistView::Created);
    }
}
