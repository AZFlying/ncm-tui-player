use crate::config::style::*;
use crate::config::Command;
use crate::ncm_client;
use crate::ui::panel::{centered_offset, PanelFocusedStatus};
use crate::ui::Controller;
use chrono::NaiveDate;
use ratatui::layout::{Margin, Rect};
use ratatui::prelude::{Constraint, Style};
use ratatui::style::palette::tailwind;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState};
use ratatui::Frame;

/// 日推列表项显示名
pub fn daily_list_label(date: NaiveDate) -> String {
    format!("每日推荐_{}", date.format("%m-%d"))
}

pub struct DailyListsPanel<'a> {
    // model
    pub focused_status: PanelFocusedStatus, // 聚焦状态交给父 screen 管理，面板自身只读不写
    //
    dates: Vec<NaiveDate>,
    dates_table_rows: Vec<Row<'a>>,
    dates_table_state: TableState,
    scrollbar_state: ScrollbarState,

    // view
    dates_table: Table<'a>,
}

impl<'a> DailyListsPanel<'a> {
    pub fn new(focused_status: PanelFocusedStatus) -> Self {
        Self {
            focused_status,
            dates: Vec::new(),
            dates_table_rows: Vec::new(),
            dates_table_state: TableState::new(),
            scrollbar_state: ScrollbarState::new(0),
            dates_table: Table::default(),
        }
    }
}

impl<'a> DailyListsPanel<'a> {
    pub fn get_selected_date(&self) -> Option<NaiveDate> {
        self.dates_table_state.selected().and_then(|selected| self.dates.get(selected)).copied()
    }
}

impl<'a> Controller for DailyListsPanel<'a> {
    async fn update_model(&mut self) -> anyhow::Result<bool> {
        let mut result = Ok(false);

        if self.dates_table_rows.is_empty() {
            let ncm_client_guard = ncm_client.lock().await;
            // 进入面板时清理窗口外的过期缓存
            ncm_client_guard.purge_daily_recommend_cache();
            self.dates = ncm_client_guard.daily_recommend_window();
            drop(ncm_client_guard);

            self.dates_table_rows = self.dates.iter().map(|date| Row::from_iter(vec![Cell::new(daily_list_label(*date))])).collect();

            // 防止悬空
            self.dates_table_state.select(None);

            self.scrollbar_state = ScrollbarState::new(self.dates_table_rows.len());

            result = Ok(true);
        }

        if self.dates_table_state.selected() == None && !self.dates_table_rows.is_empty() {
            self.dates_table_state.select(Some(0));
            self.scrollbar_state.first();
            result = Ok(true);
        }

        result
    }

    async fn handle_event(&mut self, cmd: Command) -> anyhow::Result<bool> {
        match cmd {
            Command::Down => {
                // 直接使用 select_next() 存在越界问题
                if let (Some(selected), list_len) = (self.dates_table_state.selected(), self.dates_table_rows.len()) {
                    if selected + 1 < list_len {
                        self.dates_table_state.select_next();
                        self.scrollbar_state.next();
                    }
                }
            },
            Command::Up => {
                self.dates_table_state.select_previous();
                self.scrollbar_state.prev();
            },
            Command::EnterOrPlay => {},
            Command::GoToTop => {
                self.dates_table_state.select_first();
                self.scrollbar_state.first();
            },
            Command::GoToBottom => {
                // 使用 select_last() 会越界
                self.dates_table_state.select(Some(self.dates_table_rows.len() - 1));
                self.scrollbar_state.last();
            },
            Command::SearchForward(_) => {},
            Command::SearchBackward(_) => {},
            _ => {},
        }

        Ok(true)
    }

    fn update_view(&mut self, _style: &Style) {
        let mut dates_table = Table::new(self.dates_table_rows.clone(), [Constraint::Min(20)])
            .header(Row::new(vec![Cell::new("日期")]).style(TABLE_HEADER_STYLE).height(1))
            .block({
                let mut block = Block::default()
                    .title(Line::from("每日推荐"))
                    .title_bottom(Line::from("按下`Enter`加载选中日推").centered())
                    .borders(Borders::ALL);
                if self.focused_status != PanelFocusedStatus::Nop {
                    block = block.border_style(PANEL_SELECTED_BORDER_STYLE);
                }

                block
            });

        // highlight
        if self.focused_status == PanelFocusedStatus::Inside {
            dates_table = dates_table.row_highlight_style(ITEM_SELECTED_STYLE).highlight_symbol(">")
        }

        self.dates_table = dates_table;
    }

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        let mut dates_table_state = self.dates_table_state.clone();
        if let Some(selected) = dates_table_state.selected() {
            let visible_rows = chunk.height.saturating_sub(3) as usize; // 上下边框和表头
            *dates_table_state.offset_mut() =
                centered_offset(selected, self.dates_table_rows.len(), visible_rows);
        }
        frame.render_stateful_widget(&self.dates_table, chunk, &mut dates_table_state);

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
