use crate::config::style::*;
use crate::config::Command;
use crate::ncm_client;
use crate::ui::Controller;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ncm_api::settings::{cycle_quality, quality_display_name, Settings};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};
use tui_textarea::TextArea;

#[derive(Clone, Copy)]
enum ItemKind {
    Bool,
    Quality,
    Text,
}

struct Item {
    label: &'static str,
    kind: ItemKind,
}

/// 全部 7 个设置项：0-1 API 组，2 播放组，3-6 下载组
const ITEMS: [Item; 7] = [
    Item { label: "使用远程 API", kind: ItemKind::Bool },
    Item { label: "远程 API 地址", kind: ItemKind::Text },
    Item { label: "在线播放音质", kind: ItemKind::Quality },
    Item { label: "下载目录", kind: ItemKind::Text },
    Item { label: "下载音质", kind: ItemKind::Quality },
    Item { label: "歌曲命名模板", kind: ItemKind::Text },
    Item { label: "歌词命名模板", kind: ItemKind::Text },
];

/// 每项所在组的组头标题
fn group_header(index: usize) -> &'static str {
    match index {
        0..=1 => "API",
        2 => "播放",
        _ => "下载",
    }
}

pub struct SettingsScreen<'a> {
    // model
    settings: Settings,
    selected: usize,
    editing: Option<usize>,
    status: String,

    // view
    edit_area: TextArea<'a>,
    list: List<'a>,
    /// 选中项在 list（含组头行）中的行号
    selected_line: usize,
}

impl<'a> SettingsScreen<'a> {
    pub fn new(_normal_style: &Style) -> Self {
        Self {
            settings: Settings::default(),
            selected: 0,
            editing: None,
            status: String::new(),
            edit_area: TextArea::default(),
            list: List::default(),
            selected_line: 1,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// 编辑态下接收原始按键：Enter 确认，Esc 取消，其余进入输入框
    pub async fn input(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Enter => {
                if let Some(index) = self.editing.take() {
                    let value = self.edit_area.lines()[0].clone();
                    self.set_value(index, value);
                    self.apply().await;
                }
            },
            KeyCode::Esc => {
                self.editing = None;
                self.status = String::from("已取消编辑");
            },
            _ => {
                self.edit_area.input(key_event);
            },
        }
    }

    /// 读取第 index 项的当前值（音质显示中文名）
    fn value(&self, index: usize) -> String {
        match index {
            0 => (if self.settings.use_remote_api { "开" } else { "关" }).to_string(),
            1 => self.settings.remote_api_url.clone(),
            2 => quality_display_name(&self.settings.play_quality).to_string(),
            3 => self.settings.download_path.to_string_lossy().into_owned(),
            4 => quality_display_name(&self.settings.download_quality).to_string(),
            5 => self.settings.download_file_name_pattern.clone(),
            6 => self.settings.download_lyric_name_pattern.clone(),
            _ => unreachable!(),
        }
    }

    /// 写入第 index 项的值（音质项传入的是英文 key）
    fn set_value(&mut self, index: usize, value: String) {
        match index {
            0 => self.settings.use_remote_api = value == "开",
            1 => self.settings.remote_api_url = value,
            2 => self.settings.play_quality = value,
            3 => self.settings.download_path = value.into(),
            4 => self.settings.download_quality = value,
            5 => self.settings.download_file_name_pattern = value,
            6 => self.settings.download_lyric_name_pattern = value,
            _ => unreachable!(),
        }
    }

    /// 布尔项取反 / 音质项循环一档
    async fn cycle(&mut self, forward: bool) {
        match ITEMS[self.selected].kind {
            ItemKind::Bool => {
                self.settings.use_remote_api = !self.settings.use_remote_api;
                self.apply().await;
            },
            ItemKind::Quality => {
                let key = if self.selected == 2 {
                    &self.settings.play_quality
                } else {
                    &self.settings.download_quality
                };
                let next = cycle_quality(key, forward);
                self.set_value(self.selected, next.to_string());
                self.apply().await;
            },
            ItemKind::Text => {},
        }
    }

    /// 写盘并同步内存；API 字段变化时重新连接
    async fn apply(&mut self) {
        let api_changed = ncm_client.lock().await.update_settings(self.settings.clone());
        if api_changed {
            self.status = String::from("API 配置已变化，正在重新连接…");
            if ncm_client.lock().await.check_api().await {
                self.status = String::from("已保存，API 连接正常");
            } else {
                self.status = String::from("已保存，但 API 连接失败，请检查配置");
            }
        } else {
            self.status = String::from("已保存");
        }
    }
}

impl<'a> Controller for SettingsScreen<'a> {
    async fn update_model(&mut self) -> Result<bool> {
        let settings = ncm_client.lock().await.settings();
        if settings != self.settings {
            self.settings = settings;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn handle_event(&mut self, cmd: Command) -> Result<bool> {
        if self.editing.is_some() {
            return Ok(false);
        }

        match cmd {
            Command::Up => {
                self.selected = (self.selected + ITEMS.len() - 1) % ITEMS.len();
                Ok(true)
            },
            Command::Down => {
                self.selected = (self.selected + 1) % ITEMS.len();
                Ok(true)
            },
            Command::PrevPanel => {
                self.cycle(false).await;
                Ok(true)
            },
            Command::NextPanel | Command::EnterOrPlay => match ITEMS[self.selected].kind {
                ItemKind::Text if matches!(cmd, Command::EnterOrPlay) => {
                    self.editing = Some(self.selected);
                    self.edit_area = TextArea::default();
                    let value = self.value(self.selected);
                    self.edit_area.insert_str(value);
                    self.status = String::from("编辑中：Enter 确认，Esc 取消");
                    Ok(true)
                },
                ItemKind::Text => Ok(false),
                _ => {
                    self.cycle(true).await;
                    Ok(true)
                },
            },
            _ => Ok(false),
        }
    }

    fn update_view(&mut self, _style: &Style) {
        let mut lines: Vec<ListItem> = Vec::new();
        let mut current_group = "";
        self.selected_line = 0;
        for (index, item) in ITEMS.iter().enumerate() {
            let header = group_header(index);
            if header != current_group {
                current_group = header;
                lines.push(ListItem::new(format!("─ {} ─", header)).style(Style::default().add_modifier(Modifier::BOLD)));
            }
            if index == self.selected {
                self.selected_line = lines.len();
            }
            if self.editing == Some(index) {
                // 手动渲染光标：光标处字符前后景反转（选中行的红底高亮会 patch 掉自设 bg，REVERSED 不受影响）
                let text = &self.edit_area.lines()[0];
                let col = self.edit_area.cursor().1;
                let before: String = text.chars().take(col).collect();
                let cursor_char = text.chars().nth(col).unwrap_or(' ');
                let after: String = text.chars().skip(col + 1).collect();
                lines.push(ListItem::new(Line::from(vec![
                    Span::raw(format!("  {}: {}", item.label, before)),
                    Span::styled(cursor_char.to_string(), Style::default().add_modifier(Modifier::REVERSED)),
                    Span::raw(after),
                ])));
            } else {
                lines.push(ListItem::new(format!("  {}: {}", item.label, self.value(index))));
            }
        }

        let title = format!("设置（↑↓ 选择，←→ 切换，Enter 编辑/切换） {}", self.status);
        self.list = List::new(lines)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(ITEM_SELECTED_STYLE);
    }

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        let mut state = ListState::default().with_selected(Some(self.selected_line));
        frame.render_stateful_widget(&self.list, chunk, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn renders_reversed_cursor_at_edit_position() {
        let mut screen = SettingsScreen::new(&Style::default());
        screen.settings.download_path = "/abc".into();
        screen.selected = 3;
        screen.editing = Some(3);
        screen.edit_area = TextArea::default();
        screen.edit_area.insert_str("/abc");
        // 光标移到 'b' 上
        screen.edit_area.move_cursor(tui_textarea::CursorMove::Head);
        screen.edit_area.move_cursor(tui_textarea::CursorMove::Forward);
        screen.edit_area.move_cursor(tui_textarea::CursorMove::Forward);

        screen.update_view(&Style::default());

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| screen.draw(frame, frame.area())).unwrap();

        let buffer = terminal.backend().buffer();
        let reversed: Vec<_> = buffer
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
            .collect();
        assert_eq!(reversed.len(), 1, "有且仅有一个反转样式的光标格");
        assert_eq!(reversed[0].symbol(), "b");
    }
}
