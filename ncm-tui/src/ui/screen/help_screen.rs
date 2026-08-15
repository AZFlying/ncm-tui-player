use crate::config::style::PANEL_SELECTED_BORDER_STYLE;
use crate::config::Command;
use crate::ui::Controller;
use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

#[derive(Debug, PartialEq)]
enum HelpPanel {
    Normal,
    CommandLine,
}

pub struct HelpScreen<'a> {
    // model
    focus: HelpPanel,
    normal_scroll: u16,
    cmdline_scroll: u16,
    // view
    normal_style: Style,
    normal_mode_help_text: Text<'a>,
    commandline_mode_help_text: Text<'a>,
}

impl<'a> HelpScreen<'a> {
    pub fn new(normal_style: &Style) -> Self {
        let normal_mode_help_text = Text::from(format!(
            "\
            Up:                                     {}\n\
            Down:                                   {}\n\
            Play/Pause:                             {}\n\
            Toggle Like Cursor Song:                {}\n\
            Add Highlighted Song To Play Next:      {}\n\
            Previous Panel:                         {}\n\
            Next Panel:                             {}\n\
            Go To Main Screen:                      {}\n\
            Go To Settings Screen:                  {}\n\
            Go To Help Screen (Here):               {}\n\
            Play Next Song:                         {}\n\
            Play Previous Song:                     {}\n\
            Volume Down 5%:                         {}\n\
            Volume Up 5%:                           {}\n\
            *Switch To Command Line Mode:           {}\n\
            Search Forward:                         {}\n\
            Search Backward:                        {}\n\
            New Songlist / Collect:                 {}\n\
            Delete / Unsubscribe / Remove:          {}\n\
            Switch Created/Subscribed Songlists:    {}\n\
            Pin/Unpin Highlighted Songlist:         {}\n\
            Move Pinned Songlist:                   {}\n\
            Quit:                                   {}",
            "↑ / k", "↓ / j", "\u{2423} (Space)", "l", "e", "←", "→", "1", "9", "0 / F1", ">", "<", "-", "=", ":", "/", "?", "n", "d", "c", "p", "Shift+J/K 或 Shift+↓/↑", "q",
        ));

        let commandline_mode_help_text = Text::from(format!(
            "\
            Quit:                                   {}\n\
            Switch Screen:                          {}\n\
            |_                                      {}\n\
            Go To Settings Screen:                  {}\n\
            Go To Help Screen (Here):               {}\n\
            Go To Login Screen:                     {}\n\
            Logout:                                 {}\n\
            Set Volume:                             {}\n\
            Mute:                                   {}\n\
            Set Play Mode:                          {}\n\
            |_ single play mode:                    {}\n\
            |_ single repeat mode:                  {}\n\
            |_ list repeat mode:                    {}\n\
            |_ shuffle mode:                        {}\n\
            Play Next Song:                         {}\n\
            Play Previous Song:                     {}\n\
            Like Cursor Song:                       {}\n\
            Unlike Cursor Song:                     {}\n\
            Start Auto Play:                        {}\n\
            Download Selected Song:                 {}\n\
            Download Selected Playlist:             {}\n\
            Jump To Current Song In Playlist:       {}\n\
            Jump To Top:                            {}\n\
            Jump To Bottom:                         {}\n\
            Collect Cursor Song To Songlist:        {}\n\
            Uncollect Cursor Song From Songlist:    {}\n\
            Create Songlist:                        {}\n\
            Delete Songlist:                        {}\n\
            Remove Cursor Song From Playlist:       {}\n\
            Search Forward:                         {}\n\
            Search Backward:                        {}",
            "q / quit / exit",
            "screen 0 / 1 / 2 / 3 / 9",
            "screen help / main / playlists",
            "screen settings",
            "h / help",
            "l / login",
            "logout",
            "vol / volume",
            "mute",
            "mode",
            "mode single",
            "mode sr / single-repeat",
            "mode lr / list-repeat",
            "mode s / shuf / shuffle",
            "next",
            "prev / previous",
            "like",
            "unlike",
            "start",
            "download song",
            "download playlist",
            "where this",
            "top",
            "bottom",
            "collect <歌单名>",
            "uncollect <歌单名>",
            "playlist create <名称>",
            "playlist delete <名称>",
            "remove",
            "/ xxx",
            "? xxx",
        ));

        Self {
            focus: HelpPanel::Normal,
            normal_scroll: 0,
            cmdline_scroll: 0,
            normal_style: *normal_style,
            normal_mode_help_text,
            commandline_mode_help_text,
        }
    }
}

impl<'a> Controller for HelpScreen<'a> {
    async fn update_model(&mut self) -> Result<bool> {
        Ok(false)
    }

    async fn handle_event(&mut self, cmd: Command) -> Result<bool> {
        match cmd {
            Command::Up => match self.focus {
                HelpPanel::Normal => self.normal_scroll = self.normal_scroll.saturating_sub(1),
                HelpPanel::CommandLine => self.cmdline_scroll = self.cmdline_scroll.saturating_sub(1),
            },
            Command::Down => match self.focus {
                HelpPanel::Normal => self.normal_scroll += 1,
                HelpPanel::CommandLine => self.cmdline_scroll += 1,
            },
            Command::PrevPanel | Command::NextPanel => {
                self.focus = match self.focus {
                    HelpPanel::Normal => HelpPanel::CommandLine,
                    HelpPanel::CommandLine => HelpPanel::Normal,
                };
            },
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn update_view(&mut self, _style: &Style) {}

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)].as_ref())
            .split(chunk);

        let panels = [
            (&self.normal_mode_help_text, "普通模式", self.normal_scroll, self.focus == HelpPanel::Normal, chunks[0]),
            (&self.commandline_mode_help_text, "命令行模式", self.cmdline_scroll, self.focus == HelpPanel::CommandLine, chunks[1]),
        ];

        for (text, title, scroll, focused, area) in panels {
            let mut block = Block::default()
                .title(title)
                .title_bottom(Line::from("j/k 滚动 · ←/→ 切换面板").centered())
                .borders(Borders::ALL);
            if focused {
                block = block.border_style(PANEL_SELECTED_BORDER_STYLE);
            }

            let inner_height = block.inner(area).height as usize;
            let max_scroll = text.lines.len().saturating_sub(inner_height);
            let offset = scroll.min(max_scroll as u16);

            let page = Paragraph::new(text.clone())
                .block(block)
                .style(self.normal_style)
                .scroll((offset, 0));
            frame.render_widget(page, area);

            if max_scroll > 0 {
                let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
                let mut scrollbar_state = ScrollbarState::new(max_scroll).position(offset as usize);
                frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scroll_overshoot_does_not_panic_and_focus_toggles() {
        let mut screen = HelpScreen::new(&Style::default());
        assert_eq!(screen.focus, HelpPanel::Normal);

        // 超发 Down / Up 不 panic，offset 不 underflow
        for _ in 0..100 {
            screen.handle_event(Command::Down).await.unwrap();
        }
        for _ in 0..200 {
            screen.handle_event(Command::Up).await.unwrap();
        }
        assert_eq!(screen.normal_scroll, 0);

        // 焦点往返切换
        screen.handle_event(Command::NextPanel).await.unwrap();
        assert_eq!(screen.focus, HelpPanel::CommandLine);
        screen.handle_event(Command::Down).await.unwrap();
        assert_eq!(screen.cmdline_scroll, 1);
        screen.handle_event(Command::PrevPanel).await.unwrap();
        assert_eq!(screen.focus, HelpPanel::Normal);
    }
}
