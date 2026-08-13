use crate::config::Command;
use crate::ui::Controller;
use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub struct HelpScreen<'a> {
    // view
    normal_mode_help_page: Paragraph<'a>,
    commandline_mode_help_page: Paragraph<'a>,
}

impl<'a> HelpScreen<'a> {
    pub fn new(normal_style: &Style) -> Self {
        let normal_mode_help_text = Text::from(format!(
            "\
            Up:                                     {}\n\
            Down:                                   {}\n\
            Play/Pause:                             {}\n\
            Toggle Like Cursor Song:                {}\n\
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
            New Songlist (Songlists) / Collect (Main): {}\n\
            Delete Songlist / Remove Song:          {}\n\
            Switch Created/Subscribed Songlists:    {}\n\
            Pin/Unpin Highlighted Songlist:         {}\n\
            Move Pinned Songlist:                   {}\n\
            Quit:                                   {}",
            "↑ / k", "↓ / j", "\u{2423} (Space)", "l", "←", "→", "1", "9", "0 / F1", ">", "<", "-", "=", ":", "/", "?", "n", "d", "c", "p", "Shift+J/K 或 Shift+↓/↑", "q",
        ));
        let normal_mode_help_page = Paragraph::new(normal_mode_help_text)
            .block(Block::default().title("普通模式").borders(Borders::ALL))
            .style(*normal_style);

        let commandline_mode_help_text = Text::from(format!(
            "\
            Quit:                                   {}\n\
            Switch Screen:                          {}\n\
            |_                                      {}\n\
            Go To Settings Screen:                  {}\n\
            Go To Help Screen (Here):               {}\n\
            Go To Login Screen:                     {}\n\
            Logout:                                 {}\n\
            Set Volume:                             {} (e.g. `vol 20` will set volume at 20%)\n\
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
            Start Auto Play:                        {} (Only under `list repeat mode` or `shuffle mode`)\n\
            Download Selected Song:                 {}\n\
            Download Selected Playlist:             {}\n\
            Jump To Current Song In Playlist:       {}\n\
            Jump To Top:                            {}\n\
            Jump To Bottom:                         {}\n\
            Collect Cursor Song To Songlist:        {} (候选列表自动展开)\n\
            Uncollect Cursor Song From Songlist:    {} (候选列表自动展开)\n\
            Create Songlist:                        {}\n\
            Delete Songlist:                        {} (候选列表自动展开)\n\
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
        let commandline_mode_help_page = Paragraph::new(commandline_mode_help_text)
            .block(Block::default().title("命令行模式").borders(Borders::ALL))
            .style(*normal_style);

        Self {
            normal_mode_help_page,
            commandline_mode_help_page,
        }
    }
}

impl<'a> Controller for HelpScreen<'a> {
    async fn update_model(&mut self) -> Result<bool> {
        Ok(false)
    }

    async fn handle_event(&mut self, _cmd: Command) -> Result<bool> {
        Ok(false)
    }

    fn update_view(&mut self, _style: &Style) {}

    fn draw(&self, frame: &mut Frame, chunk: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)].as_ref())
            .split(chunk);

        frame.render_widget(&self.normal_mode_help_page, chunks[0]);

        frame.render_widget(&self.commandline_mode_help_page, chunks[1]);
    }
}
