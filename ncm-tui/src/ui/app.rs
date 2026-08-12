use crate::config::style::*;
use crate::config::LOGO_LINES;
use crate::ui::widget::{BottomBar, CommandLine};
use crate::{
    actions, command_queue,
    config::{AppMode, Command, ScreenEnum},
    ncm_client, player,
    ui::{screen::*, Controller},
};
use anyhow::Result;
use crossterm::event::KeyModifiers;
use crossterm::{
    event,
    event::{Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use log::debug;
use ncm_api::model::Songlist;
use ratatui::prelude::*;
use ratatui::style::palette::tailwind;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use std::io::Stdout;
use unicode_width::UnicodeWidthStr;

/// 命令补全候选列表状态
struct CompletionState {
    /// 命令前缀，如 "collect "
    prefix: String,
    /// 过滤与排序后的候选歌单名
    candidates: Vec<String>,
    /// 高亮索引
    selected: usize,
}

/// 待确认操作，按 y 执行、其余任意键取消
enum PendingConfirm {
    DeleteSonglist { id: u64, name: String },
    RemoveSong { songlist_id: u64, song_id: u64, song_name: String },
}

pub struct App<'a> {
    // model
    current_screen: ScreenEnum,
    current_mode: AppMode,
    need_re_update_view: bool,
    /// 待确认操作（删除歌单/移除歌曲）
    pending_confirm: Option<PendingConfirm>,
    /// 命令补全候选列表，输入不匹配补全前缀或无候选时为 None
    completion: Option<CompletionState>,

    // view
    main_screen: MainScreen<'a>,
    settings_screen: SettingsScreen<'a>,
    songlists_screen: SonglistsScreen<'a>,
    daily_screen: DailyScreen<'a>,
    login_screen: LoginScreen<'a>,
    help_screen: HelpScreen<'a>,
    command_line: CommandLine<'a>,
    bottom_bar: BottomBar<'a>,

    // const
    terminal: Terminal<CrosstermBackend<Stdout>>,
    normal_style: Style,
}

/// public
impl<'a> App<'a> {
    pub fn new(terminal: Terminal<CrosstermBackend<Stdout>>) -> Self {
        let normal_style = Style::default();

        Self {
            current_screen: ScreenEnum::Launch,
            current_mode: AppMode::Normal,
            need_re_update_view: true,
            pending_confirm: None,
            completion: None,
            main_screen: MainScreen::new(&normal_style),
            settings_screen: SettingsScreen::new(&normal_style),
            songlists_screen: SonglistsScreen::new(&normal_style),
            daily_screen: DailyScreen::new(&normal_style),
            login_screen: LoginScreen::new(&normal_style),
            help_screen: HelpScreen::new(&normal_style),
            command_line: CommandLine::new(),
            bottom_bar: BottomBar::new(&normal_style),
            terminal,
            normal_style,
        }
    }

    /// 绘制启动第一帧（网易云logo）
    pub fn draw_launch_screen(&mut self) -> Result<()> {
        let mut logo_lines = Vec::new();
        for logo_line in LOGO_LINES {
            // 为 logo 绘制2格阴影
            let mut line_spans = Vec::new();
            let mut next_space_is_shadow = false;
            let mut shadow_count = 0;
            for char in logo_line.chars() {
                match char {
                    ' ' => {
                        if next_space_is_shadow {
                            line_spans.push(Span::from("▇").fg(Color::Rgb(255, 136, 136)));

                            shadow_count += 1;
                            if shadow_count == 2 {
                                shadow_count = 0;
                                next_space_is_shadow = false;
                            }
                        } else {
                            line_spans.push(Span::from(" "));
                        }
                    },
                    '▇' => {
                        line_spans.push(Span::from("▇").fg(tailwind::WHITE));

                        shadow_count = 0;
                        next_space_is_shadow = true;
                    },
                    _ => {},
                }
            }

            //
            logo_lines.push(Line::from(line_spans).centered());
        }
        let logo_lines_count = logo_lines.len();

        // 绘制
        self.terminal.draw(|frame| {
            let chunk = frame.area();

            // 竖直居中
            let available_line_count = chunk.height as usize;
            if available_line_count > logo_lines_count {
                for _ in 0..(available_line_count - logo_lines_count) / 2 {
                    logo_lines.insert(0, Line::from(""));
                }
            }

            let logo_paragraph = Paragraph::new(logo_lines).bg(tailwind::RED.c500);

            frame.render_widget(logo_paragraph, chunk);
        })?;

        Ok(())
    }

    /// cookie 登录/二维码登录后均调用
    pub async fn init_after_login(&mut self) -> Result<()> {
        // 初始化，获取用户所有歌单（缩略）和 `用户喜欢的音乐` 歌单（详细信息）
        actions::init_songlists().await?;

        // 提醒 main_screen 更新 playlist
        command_queue.lock().await.push_back(Command::RefreshPlaylist);

        // 切换到 main_screen
        self.switch_screen(ScreenEnum::Main).await;

        Ok(())
    }

    /// 尝试 cookie 登录失败后调用
    pub async fn init_after_no_login(&mut self) {
        self.switch_screen(ScreenEnum::Main).await;
        self.command_line.set_content("按下`:`进行命令输入，输入`login`命令进入登录页面");
    }

    pub fn restore_terminal(&mut self) -> Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;

        Ok(())
    }
}

/// app routine （与 Controller 略有区别）
impl<'a> App<'a> {
    pub async fn update_model(&mut self) -> Result<()> {
        // screen
        self.need_re_update_view = match self.current_screen {
            ScreenEnum::Help => false,
            ScreenEnum::Login => self.update_login_model().await?,
            ScreenEnum::Main => self.main_screen.update_model().await?,
            ScreenEnum::Settings => self.settings_screen.update_model().await?,
            ScreenEnum::Songlists => self.songlists_screen.update_model().await?,
            ScreenEnum::Daily => self.daily_screen.update_model().await?,
            _ => false,
        };

        // bottom_bar
        self.bottom_bar.update_model().await?;

        Ok(())
    }

    /// 解析命令
    pub async fn parse_key_to_event(&mut self) -> Result<()> {
        if let Event::Key(key_event) = event::read()? {
            if key_event.kind == KeyEventKind::Press || key_event.kind == KeyEventKind::Repeat {
                // 有待确认操作时：y 执行，其余任意键取消
                if self.pending_confirm.is_some() {
                    if matches!(key_event.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                        self.execute_confirmed().await;
                    } else {
                        self.pending_confirm = None;
                        self.command_line.set_content("已取消");
                    }
                    return Ok(());
                }

                match (&self.current_mode, key_event.code) {
                    // Normal 模式
                    (AppMode::Normal, _) => {
                        // 设置屏编辑态：原始按键直接交给设置屏的输入框
                        if self.current_screen == ScreenEnum::Settings && self.settings_screen.is_editing() {
                            self.settings_screen.input(key_event).await;
                            self.need_re_update_view = true;
                        } else {
                            self.get_command_from_key(key_event.modifiers, key_event.code).await;
                        }
                    },

                    // Search 模式
                    // 响应 n / N / esc / enter
                    (AppMode::Search(search_keywords), KeyCode::Char('n')) => {
                        command_queue.lock().await.push_back(Command::SearchForward(search_keywords.clone()));
                    },
                    (AppMode::Search(search_keywords), KeyCode::Char('N')) => {
                        command_queue.lock().await.push_back(Command::SearchBackward(search_keywords.clone()));
                    },
                    (AppMode::Search(_), KeyCode::Esc) => {
                        self.back_to_normal_mode();
                    },
                    (AppMode::Search(_), KeyCode::Enter | KeyCode::Char(':')) => {
                        // 返回 normal 模式，同时解析对应的命令，后续执行
                        self.back_to_normal_mode();
                        self.get_command_from_key(key_event.modifiers, key_event.code).await;
                    },
                    (AppMode::Search(_), KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j')) => {
                        // 不返回 normal 模式，同时解析对应的命令，后续执行
                        self.get_command_from_key(key_event.modifiers, key_event.code).await;
                    },
                    (AppMode::Search(_), _) => {},

                    // CommandLine 模式
                    (AppMode::CommandLine, KeyCode::Enter) => {
                        if let Some(state) = self.completion.take() {
                            // 填入高亮候选并关闭列表，再次 Enter 才执行
                            let completed = format!("{}{}", state.prefix, state.candidates[state.selected]);
                            self.command_line.set_content(&completed);
                        } else {
                            self.parse_command().await;
                        }
                    },
                    (AppMode::CommandLine, KeyCode::Esc) => {
                        // 列表展开时仅关闭列表
                        if self.completion.take().is_none() {
                            self.back_to_normal_mode();
                        }
                    },
                    (AppMode::CommandLine, KeyCode::Tab | KeyCode::Down) => {
                        if let Some(state) = &mut self.completion {
                            state.selected = (state.selected + 1) % state.candidates.len();
                        } else if key_event.code == KeyCode::Down {
                            self.command_line.input(key_event);
                        }
                    },
                    (AppMode::CommandLine, KeyCode::BackTab | KeyCode::Up) => {
                        if let Some(state) = &mut self.completion {
                            state.selected = (state.selected + state.candidates.len() - 1) % state.candidates.len();
                        } else if key_event.code == KeyCode::Up {
                            self.command_line.input(key_event);
                        }
                    },
                    (AppMode::CommandLine, KeyCode::Backspace) => {
                        if self.command_line.is_content_empty() {
                            self.back_to_normal_mode();
                        } else {
                            self.command_line.input(key_event);
                            self.refresh_completion().await;
                        }
                    },
                    (AppMode::CommandLine, _) => {
                        self.command_line.input(key_event);
                        self.refresh_completion().await;
                    },
                }
            }
        }

        Ok(())
    }

    /// 事件处理（事件包括按键触发的事件和程序中某部分自行产生的事件）
    pub async fn handle_event(&mut self) -> Result<bool> {
        let mut command_queue_guard = command_queue.lock().await;
        if let Some(cmd) = command_queue_guard.pop_front() {
            if !command_queue_guard.is_empty() {
                debug!("command queue: {:?}", command_queue_guard);
            }

            // 避免死锁
            drop(command_queue_guard);

            // app响应的事件
            match cmd.clone() {
                Command::Quit => {
                    return Ok(false);
                },
                Command::GotoScreen(to_screen) => {
                    self.switch_screen(to_screen).await;
                    self.command_line.handle_event(cmd.clone()).await?;
                },
                Command::EnterCommand => {
                    self.switch_to_command_line_mode();
                },
                Command::Logout => {
                    self.login_screen = LoginScreen::new(&self.normal_style);
                    // TODO: 清除 cache
                    ncm_client.lock().await.logout().await?;
                },
                Command::PlayOrPause => {
                    player.lock().await.play_or_pause();
                },
                Command::SetVolume(vol) => {
                    player.lock().await.set_volume(vol);
                },
                Command::VolumeUp => {
                    let mut player_guard = player.lock().await;
                    let vol = player_guard.volume() + 0.05;
                    player_guard.set_volume(vol);
                },
                Command::VolumeDown => {
                    let mut player_guard = player.lock().await;
                    let vol = player_guard.volume() - 0.05;
                    player_guard.set_volume(vol);
                },
                Command::SwitchPlayMode(play_mode) => {
                    player.lock().await.set_play_mode(play_mode);
                },
                Command::StartPlay => {
                    let mut player_guard = player.lock().await;
                    if let Err(e) = player_guard.start_play(ncm_client.lock().await).await {
                        self.command_line.set_content(e.to_string().as_str());
                    } else {
                        // 光标跳转到播放歌曲，并将焦点移到 playlist 面板
                        command_queue.lock().await.push_back(Command::WhereIsThisSong);
                    }
                },
                Command::NextSong => {
                    let mut player_guard = player.lock().await;
                    let prev_index = player_guard.current_song_index();
                    player_guard.play_next_song_now(ncm_client.lock().await).await?;
                    // 切歌被防抖拦截时索引不变，不移动光标
                    if player_guard.current_song_index() != prev_index {
                        command_queue.lock().await.push_back(Command::SyncPlaylistCursor);
                    }
                },
                Command::PrevSong => {
                    let mut player_guard = player.lock().await;
                    let prev_index = player_guard.current_song_index();
                    player_guard.play_prev_song_now(ncm_client.lock().await).await?;
                    if player_guard.current_song_index() != prev_index {
                        command_queue.lock().await.push_back(Command::SyncPlaylistCursor);
                    }
                },
                Command::SetCurrentSongLiked(requested) => {
                    let song = player.lock().await.active_song();
                    if let Some(song) = song {
                        let (like, result) = {
                            let mut ncm_client_guard = ncm_client.lock().await;
                            let like = requested.unwrap_or(!ncm_client_guard.is_song_liked(song.id));
                            let result = ncm_client_guard.like_song(song.id, like).await;
                            (like, result)
                        };
                        match result {
                            Ok(()) => {
                                player.lock().await.set_song_liked(song.id, like);
                                command_queue.lock().await.push_back(Command::RefreshPlaylist);
                                self.command_line.set_content(
                                    format!("{}：{}", if like { "已喜欢" } else { "已取消喜欢" }, song.name).as_str(),
                                );
                            },
                            Err(err) => self.command_line.set_content(err.to_string().as_str()),
                        }
                    } else {
                        self.command_line.set_content("当前没有正在播放或暂停的歌曲");
                    }
                },
                Command::DownloadSong => {
                    let song = match self.current_screen {
                        ScreenEnum::Main => self.main_screen.selected_song(),
                        ScreenEnum::Songlists => self.songlists_screen.selected_song(),
                        ScreenEnum::Daily => self.daily_screen.selected_song(),
                        _ => None,
                    };
                    if let Some(song) = song {
                        self.command_line.set_content(format!("已开始下载歌曲《{}》", song.name).as_str());
                        actions::download_song(song);
                    } else {
                        self.command_line.set_content("当前界面没有已选择的歌曲");
                    }
                },
                Command::DownloadPlaylist => match self.current_screen {
                    ScreenEnum::Main => {
                        let (name, songs) = {
                            let player_guard = player.lock().await;
                            (player_guard.current_playlist_name().clone(), player_guard.current_playlist().clone())
                        };
                        if songs.is_empty() {
                            self.command_line.set_content("当前播放列表为空");
                        } else {
                            self.command_line.set_content(format!("已开始下载歌单《{}》", name).as_str());
                            actions::download_playlist(name, songs);
                        }
                    },
                    ScreenEnum::Songlists => {
                        if let Some(songlist) = self.songlists_screen.selected_songlist() {
                            self.command_line.set_content(format!("已开始下载歌单《{}》", songlist.name).as_str());
                            actions::download_unloaded_playlist(songlist);
                        } else {
                            self.command_line.set_content("当前没有已选择的歌单");
                        }
                    },
                    ScreenEnum::Daily => match self.daily_screen.selected_songlist() {
                        Some(songlist) if !songlist.songs.is_empty() => {
                            self.command_line.set_content(format!("已开始下载歌单《{}》", songlist.name).as_str());
                            actions::download_playlist(songlist.name.clone(), songlist.songs.clone());
                        },
                        _ => self.command_line.set_content("请先在日推屏加载要下载的日推"),
                    },
                    _ => self.command_line.set_content("当前界面不支持下载歌单"),
                },
                Command::DownloadFinished(message) => {
                    self.command_line.set_content(&message);
                },
                Command::ShowMessage(message) => {
                    self.command_line.set_content(&message);
                },
                Command::CollectToSonglist(name) => {
                    self.update_song_collection(name, true).await;
                },
                Command::UncollectFromSonglist(name) => {
                    self.update_song_collection(name, false).await;
                },
                Command::CreateSonglist(name) => {
                    let result = {
                        let ncm_client_guard = ncm_client.lock().await;
                        ncm_client_guard.create_songlist(&name).await
                    };
                    match result {
                        Ok(new_id) => {
                            // 本地插入新歌单（紧随「我喜欢的音乐」之后，与服务端排序一致）
                            let creator = ncm_client.lock().await.login_account().map(|a| a.nickname).unwrap_or_default();
                            if let Some(id) = new_id {
                                let mut player_guard = player.lock().await;
                                let insert_pos = match player_guard.songlists().first() {
                                    Some(first) if first.special_type == 5 => 1,
                                    _ => 0,
                                };
                                player_guard.songlists_mut().insert(
                                    insert_pos,
                                    Songlist {
                                        name: name.clone(),
                                        id,
                                        songs_count: 0,
                                        creator,
                                        subscribed: false,
                                        special_type: 0,
                                        songs: Vec::new(),
                                    },
                                );
                            } else if let Ok(songlists) = ncm_client.lock().await.get_user_all_songlists().await {
                                // 未解析到新歌单 id，退化为全量刷新
                                player.lock().await.set_songlists(songlists);
                            }
                            command_queue.lock().await.push_back(Command::RefreshPlaylist);
                            self.command_line.set_content(format!("已创建歌单《{}》", name).as_str());
                        },
                        Err(err) => self.command_line.set_content(err.to_string().as_str()),
                    }
                },
                Command::DeleteSonglistByName(name) => {
                    let target = {
                        let player_guard = player.lock().await;
                        player_guard.songlists().iter().find(|sl| sl.name == name).cloned()
                    };
                    match target {
                        None => self.command_line.set_content(format!("未找到歌单《{}》", name).as_str()),
                        Some(sl) if sl.subscribed => self.command_line.set_content("不能删除收藏的歌单"),
                        Some(sl) if sl.special_type == 5 => self.command_line.set_content("不能删除「我喜欢的音乐」"),
                        Some(sl) => {
                            self.pending_confirm = Some(PendingConfirm::DeleteSonglist { id: sl.id, name: sl.name.clone() });
                            self.command_line.set_content(format!("删除歌单《{}》？[y/N]", sl.name).as_str());
                        },
                    }
                },
                Command::RemoveSongFromSonglist { songlist_id, songlist_name, song_id, song_name } => {
                    self.pending_confirm = Some(PendingConfirm::RemoveSong { songlist_id, song_id, song_name: song_name.clone() });
                    self.command_line.set_content(format!("从《{}》移除《{}》？[y/N]", songlist_name, song_name).as_str());
                },
                Command::RemoveFromCurrentPlaylist => {
                    self.request_remove_cursor_song().await;
                },
                Command::Delete if self.current_screen == ScreenEnum::Main => {
                    self.request_remove_cursor_song().await;
                },
                Command::NewOrCollect => {
                    match self.current_screen {
                        ScreenEnum::Songlists => {
                            self.switch_to_command_line_mode();
                            self.command_line.set_content("playlist create ");
                        },
                        ScreenEnum::Main | ScreenEnum::Daily => {
                            self.switch_to_command_line_mode();
                            self.command_line.set_content("collect ");
                            self.refresh_completion().await;
                        },
                        _ => {},
                    }
                },
                Command::SearchForward(search_keywords) => {
                    self.switch_to_search_mode(search_keywords);
                },
                Command::SearchBackward(search_keywords) => {
                    self.switch_to_search_mode(search_keywords);
                },
                _ => {},
            }

            // 需要向下传递的事件
            if matches!(
                cmd,
                Command::Down
                    | Command::Up
                    | Command::NextPanel
                    | Command::PrevPanel
                    | Command::Esc
                    | Command::EnterOrPlay
                    | Command::Play
                    | Command::WhereIsThisSong
                    | Command::SyncPlaylistCursor
                    | Command::GoToTop
                    | Command::GoToBottom
                    | Command::Delete
                    | Command::SongRemovalDone { .. }
                    | Command::SearchForward(_)
                    | Command::SearchBackward(_)
                    | Command::RefreshPlaylist
            ) {
                // 先 update_model(), 再 handle_event()
                // 取或值
                // 若写成 self.need_re_update_view = self.need_re_update_view || match ... {} ，match块内的方法可能不被执行
                self.need_re_update_view = match self.current_screen {
                    ScreenEnum::Main => self.main_screen.handle_event(cmd).await?,
                    ScreenEnum::Settings => self.settings_screen.handle_event(cmd).await?,
                    ScreenEnum::Songlists => self.songlists_screen.handle_event(cmd).await?,
                    ScreenEnum::Daily => self.daily_screen.handle_event(cmd).await?,
                    ScreenEnum::Login => self.login_screen.handle_event(cmd).await?,
                    ScreenEnum::Help => self.help_screen.handle_event(cmd).await?,
                    _ => false,
                } || self.need_re_update_view;
            }
        }

        Ok(true)
    }

    pub fn update_view(&mut self) {
        // screen 只在 need_re_update_view 为 true 时更新view
        if self.need_re_update_view {
            match self.current_screen {
                ScreenEnum::Help => {},
                ScreenEnum::Login => self.login_screen.update_view(&self.normal_style),
                ScreenEnum::Main => self.main_screen.update_view(&self.normal_style),
                ScreenEnum::Settings => self.settings_screen.update_view(&self.normal_style),
                ScreenEnum::Songlists => self.songlists_screen.update_view(&self.normal_style),
                ScreenEnum::Daily => self.daily_screen.update_view(&self.normal_style),
                _ => {},
            }
        }

        // bottom_bar
        self.bottom_bar.update_view(&self.normal_style);

        // command_line
        self.command_line.update_view(&self.normal_style);
    }

    pub fn draw(&mut self) -> Result<()> {
        // Launch Screen 需要全屏绘制
        if self.current_screen == ScreenEnum::Launch {
            self.draw_launch_screen()?;
            return Ok(());
        }

        self.update_view();

        self.terminal.draw(|frame| {
            // 分割
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(1)].as_ref())
                .split(frame.area());

            // 渲染 screen
            match self.current_screen {
                ScreenEnum::Help => self.help_screen.draw(frame, chunks[0]),
                ScreenEnum::Login => self.login_screen.draw(frame, chunks[0]),
                ScreenEnum::Main => self.main_screen.draw(frame, chunks[0]),
                ScreenEnum::Settings => self.settings_screen.draw(frame, chunks[0]),
                ScreenEnum::Songlists => self.songlists_screen.draw(frame, chunks[0]),
                ScreenEnum::Daily => self.daily_screen.draw(frame, chunks[0]),
                _ => {},
            }

            // 渲染 bottom_bar
            self.bottom_bar.draw(frame, chunks[1]);

            // 渲染 command_line
            self.command_line.draw(frame, chunks[2]);

            // 渲染补全候选列表（浮于命令行上方）
            if let Some(state) = &self.completion {
                let area = frame.area();
                let height = state.candidates.len().min(10) as u16 + 2;
                let max_name_width = state.candidates.iter().map(|n| UnicodeWidthStr::width(n.as_str())).max().unwrap_or(0) as u16;
                let width = (max_name_width + 4).clamp(20, area.width.saturating_sub(2));
                let popup_area = Rect::new(0, area.height.saturating_sub(1 + height), width, height);

                let items: Vec<ListItem> = state.candidates.iter().map(|n| ListItem::new(n.clone())).collect();
                let list = List::new(items)
                    .block(Block::default().title("选择歌单").borders(Borders::ALL))
                    .highlight_style(ITEM_SELECTED_STYLE);
                let mut list_state = ListState::default().with_selected(Some(state.selected));

                frame.render_widget(Clear, popup_area);
                frame.render_stateful_widget(list, popup_area, &mut list_state);
            }
        })?;

        Ok(())
    }
}

/// private
impl<'a> App<'a> {
    async fn get_command_from_key(&mut self, key_modifiers: KeyModifiers, key_code: KeyCode) {
        let cmd = match key_code {
            KeyCode::Down => Command::Down,
            KeyCode::Char('j') => Command::Down,
            KeyCode::Up => Command::Up,
            KeyCode::Char('k') => Command::Up,
            KeyCode::Char(' ') => Command::PlayOrPause,
            KeyCode::Enter => {
                if key_modifiers.contains(KeyModifiers::ALT) {
                    Command::Play
                } else {
                    Command::EnterOrPlay
                }
            },
            KeyCode::Esc => Command::Esc,
            KeyCode::Right => Command::NextPanel,
            KeyCode::Char('l') => Command::SetCurrentSongLiked(None),
            KeyCode::Left => Command::PrevPanel,
            KeyCode::Char('h') => Command::PrevPanel,
            KeyCode::Char('1') => Command::GotoScreen(ScreenEnum::Main),
            KeyCode::Char('2') => Command::GotoScreen(ScreenEnum::Songlists),
            KeyCode::Char('3') => Command::GotoScreen(ScreenEnum::Daily),
            KeyCode::Char('9') => Command::GotoScreen(ScreenEnum::Settings),
            KeyCode::Char('0') => Command::GotoScreen(ScreenEnum::Help),
            KeyCode::F(1) => Command::GotoScreen(ScreenEnum::Help),
            KeyCode::Char('.') | KeyCode::Char('。') => Command::NextSong,
            KeyCode::Char(',') | KeyCode::Char('，') => Command::PrevSong,
            KeyCode::Char(':') | KeyCode::Char('：') => Command::EnterCommand,
            KeyCode::Char('/') => {
                self.switch_to_search_input_mode();
                self.command_line.set_content("/ ");
                Command::Nop
            },
            KeyCode::Char('?') | KeyCode::Char('？') => {
                self.switch_to_search_input_mode();
                self.command_line.set_content("? ");
                Command::Nop
            },
            KeyCode::Char('-') => Command::VolumeDown,
            KeyCode::Char('=') => Command::VolumeUp,
            KeyCode::Char('n') => Command::NewOrCollect,
            KeyCode::Char('d') => Command::Delete,
            //
            KeyCode::Tab => Command::NextPanel,
            KeyCode::BackTab => Command::PrevPanel,
            KeyCode::Char('q') => Command::Quit,
            _ => Command::Nop,
        };

        command_queue.lock().await.push_back(cmd);
    }

    async fn parse_command(&mut self) {
        let input_cmd = self.command_line.get_content();

        self.back_to_normal_mode();

        match Command::parse(&input_cmd) {
            Ok(cmd) => {
                command_queue.lock().await.push_back(cmd);
            },
            Err(e) => {
                self.command_line.set_content(format!("{e}").as_str());
            },
        }
    }

    fn back_to_normal_mode(&mut self) {
        self.current_mode = AppMode::Normal;
        self.completion = None;
        self.command_line.set_to_normal_mode();
    }

    fn switch_to_command_line_mode(&mut self) {
        self.current_mode = AppMode::CommandLine;
        self.completion = None;
        self.command_line.set_to_command_line_mode();
    }

    fn switch_to_search_mode(&mut self, search_keywords: Vec<String>) {
        self.current_mode = AppMode::Search(search_keywords);
        self.command_line.set_to_search_mode()
    }

    /// 输入搜索命令时特殊的混合模式
    fn switch_to_search_input_mode(&mut self) {
        self.current_mode = AppMode::CommandLine;
        self.command_line.set_to_search_mode();
    }

    /// 收藏/取消收藏当前播放歌曲到指定歌单
    async fn update_song_collection(&mut self, name: String, add: bool) {
        let (song, target) = {
            let player_guard = player.lock().await;
            let song = player_guard.active_song();
            let target = player_guard
                .songlists()
                .iter()
                .find(|sl| sl.name == name && !sl.subscribed && sl.special_type != 5)
                .cloned();
            (song, target)
        };

        let Some(song) = song else {
            self.command_line.set_content("当前没有正在播放或暂停的歌曲");
            return;
        };
        let Some(songlist) = target else {
            self.command_line.set_content(format!("未找到可操作的歌单《{}》（仅支持自建歌单）", name).as_str());
            return;
        };

        let result = {
            let ncm_client_guard = ncm_client.lock().await;
            ncm_client_guard.update_songlist_tracks(add, songlist.id, song.id).await
        };
        match result {
            Ok(()) => {
                // 本地同步歌单歌曲计数，避免服务端缓存导致的刷新滞后
                let mut player_guard = player.lock().await;
                if let Some(sl) = player_guard.songlists_mut().iter_mut().find(|sl| sl.id == songlist.id) {
                    if add {
                        sl.songs_count += 1;
                    } else {
                        sl.songs_count = sl.songs_count.saturating_sub(1);
                    }
                }
                drop(player_guard);
                command_queue.lock().await.push_back(Command::RefreshPlaylist);
                self.command_line.set_content(
                    format!("{}：《{}》→《{}》", if add { "已收藏" } else { "已取消收藏" }, song.name, songlist.name).as_str(),
                );
            },
            Err(err) => self.command_line.set_content(err.to_string().as_str()),
        }
    }

    /// 请求从当前播放列表移除光标歌曲（进入确认流）
    async fn request_remove_cursor_song(&mut self) {
        let Some(song) = self.main_screen.selected_song() else {
            self.command_line.set_content("当前没有已选择的歌曲");
            return;
        };
        let songlist = {
            let player_guard = player.lock().await;
            match player_guard.current_playlist_id() {
                Some(playlist_id) => player_guard.songlists().iter().find(|sl| sl.id == playlist_id).cloned(),
                None => None,
            }
        };
        match songlist {
            Some(sl) if sl.subscribed => self.command_line.set_content("不能从收藏的歌单移除歌曲"),
            Some(sl) if sl.special_type == 5 => self.command_line.set_content("不能从「我喜欢的音乐」移除歌曲"),
            Some(sl) => {
                command_queue.lock().await.push_back(Command::RemoveSongFromSonglist {
                    songlist_id: sl.id,
                    songlist_name: sl.name,
                    song_id: song.id,
                    song_name: song.name,
                });
            },
            None => self.command_line.set_content("当前播放列表不支持移除歌曲"),
        }
    }

    /// 执行已确认的操作
    async fn execute_confirmed(&mut self) {
        match self.pending_confirm.take() {
            Some(PendingConfirm::DeleteSonglist { id, name }) => {
                let result = {
                    let ncm_client_guard = ncm_client.lock().await;
                    ncm_client_guard.delete_songlist(id).await
                };
                match result {
                    Ok(()) => {
                        player.lock().await.songlists_mut().retain(|sl| sl.id != id);
                        command_queue.lock().await.push_back(Command::RefreshPlaylist);
                        self.command_line.set_content(format!("已删除歌单《{}》", name).as_str());
                    },
                    Err(err) => self.command_line.set_content(err.to_string().as_str()),
                }
            },
            Some(PendingConfirm::RemoveSong { songlist_id, song_id, song_name }) => {
                let result = {
                    let ncm_client_guard = ncm_client.lock().await;
                    ncm_client_guard.update_songlist_tracks(false, songlist_id, song_id).await
                };
                match result {
                    Ok(()) => {
                        // 同步歌单计数
                        let mut player_guard = player.lock().await;
                        if let Some(sl) = player_guard.songlists_mut().iter_mut().find(|sl| sl.id == songlist_id) {
                            sl.songs_count = sl.songs_count.saturating_sub(1);
                        }
                        drop(player_guard);
                        // 远程成功，转发给屏幕执行本地移除
                        command_queue.lock().await.push_back(Command::SongRemovalDone { songlist_id, song_id });
                        self.command_line.set_content(format!("已从歌单移除：《{}》", song_name).as_str());
                    },
                    Err(err) => self.command_line.set_content(err.to_string().as_str()),
                }
            },
            None => {},
        }
    }

    /// 根据当前命令行输入刷新补全候选列表
    async fn refresh_completion(&mut self) {
        const PREFIXES: [&str; 3] = ["collect ", "uncollect ", "playlist delete "];
        let content = self.command_line.get_content();

        self.completion = None;
        let Some(prefix) = PREFIXES.iter().find(|p| content.starts_with(**p)) else {
            return;
        };
        let arg = &content[prefix.len()..];
        let names: Vec<String> = player
            .lock()
            .await
            .songlists()
            .iter()
            .filter(|sl| !sl.subscribed && sl.special_type != 5)
            .map(|sl| sl.name.clone())
            .collect();
        let candidates = filter_candidates(arg, &names);
        if candidates.is_empty() {
            return;
        }

        self.completion = Some(CompletionState { prefix: prefix.to_string(), candidates, selected: 0 });
    }

    async fn update_login_model(&mut self) -> Result<bool> {
        //
        let need_redraw = self.login_screen.update_model().await?;

        if ncm_client.lock().await.is_login() {
            // 登录成功
            self.init_after_login().await?;
            Ok(true)
        } else {
            Ok(need_redraw)
        }
    }

    async fn switch_screen(&mut self, to_screen: ScreenEnum) {
        // 已登录状态不能切换到 login_screen
        let ncm_client_guard = ncm_client.lock().await;
        if to_screen == ScreenEnum::Login && ncm_client_guard.is_login() {
            if let Some(login_account) = ncm_client_guard.login_account() {
                self.command_line
                    .set_content(format!("正在使用`{}`账号，请先使用`logout`命令登出当前账号", login_account.nickname).as_str());
            } else {
                self.command_line.set_content("请先使用`logout`命令登出当前账号");
            }

            return;
        }
        drop(ncm_client_guard);

        // 切换到 main_screen 时显示提示
        if to_screen == ScreenEnum::Main {
            self.command_line.set_content("按0或F1键查看help页面");
        }

        // 切换到 main_screen 时释放当前屏幕（节省内存开销）
        match self.current_screen {
            ScreenEnum::Login => {
                self.login_screen = LoginScreen::new(&self.normal_style);
            },
            ScreenEnum::Songlists => {
                self.songlists_screen = SonglistsScreen::new(&self.normal_style);
            },
            ScreenEnum::Daily => {
                self.daily_screen = DailyScreen::new(&self.normal_style);
            },
            _ => {},
        }

        self.need_re_update_view = true;
        self.current_screen = to_screen;
    }
}

/// 过滤候选：不区分大小写，前缀匹配优先于中间子串匹配，同级保持原顺序
fn filter_candidates(arg: &str, names: &[String]) -> Vec<String> {
    let arg = arg.to_lowercase();
    let mut prefix_matches = Vec::new();
    let mut substring_matches = Vec::new();
    for name in names {
        let lower = name.to_lowercase();
        if lower.starts_with(&arg) {
            prefix_matches.push(name.clone());
        } else if lower.contains(&arg) {
            substring_matches.push(name.clone());
        }
    }
    prefix_matches.extend(substring_matches);
    prefix_matches
}

#[cfg(test)]
mod tests {
    use super::filter_candidates;

    fn names() -> Vec<String> {
        vec!["JPOP".to_string(), "跑步精选".to_string(), "去跑步".to_string(), "KTV".to_string()]
    }

    #[test]
    fn filters_case_insensitively() {
        assert_eq!(filter_candidates("jpop", &names()), vec!["JPOP"]);
        assert_eq!(filter_candidates("K", &names()), vec!["KTV"]);
    }

    #[test]
    fn matches_substring_anywhere() {
        assert_eq!(filter_candidates("跑步", &names()), vec!["跑步精选", "去跑步"]);
    }

    #[test]
    fn ranks_prefix_matches_first() {
        assert_eq!(filter_candidates("跑", &names()), vec!["跑步精选", "去跑步"]);
    }

    #[test]
    fn empty_input_lists_all() {
        assert_eq!(filter_candidates("", &names()).len(), 4);
        assert!(filter_candidates("不存在的歌单", &names()).is_empty());
    }
}
