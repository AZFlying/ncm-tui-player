use crate::config::Command;
use crate::{command_queue, ncm_client, player};
use anyhow::Result;
use ncm_api::model::{Song, Songlist};
use ncm_api::DownloadResult;

pub async fn init_songlists() -> Result<()> {
    let mut ncm_client_guard = ncm_client.lock().await;
    ncm_client_guard.load_liked_song_ids().await?;
    let mut player_guard = player.lock().await;
    if let Ok(songlists) = ncm_client_guard.get_user_all_songlists().await {
        let len = songlists.len();

        player_guard.set_songlists(songlists);

        if len > 0 {
            player_guard.switch_playlist(0, ncm_client_guard).await?;
        }
    }

    Ok(())
}

pub fn download_song(song: Song) {
    spawn_download(format!("歌曲《{}》", song.name), vec![song]);
}

pub fn download_playlist(name: String, songs: Vec<Song>) {
    spawn_download(format!("歌单《{}》", name), songs);
}

pub fn download_unloaded_playlist(songlist: Songlist) {
    tokio::spawn(async move {
        let name = songlist.name.clone();
        let load = { ncm_client.lock().await.load_songlist(songlist) };
        match load.await {
            Ok(songlist) => download_all(format!("歌单《{}》", songlist.name), songlist.songs).await,
            Err(err) => send_download_message(format!("下载歌单《{}》失败：{}", name, err)).await,
        }
    });
}

fn spawn_download(label: String, songs: Vec<Song>) {
    tokio::spawn(download_all(label, songs));
}

async fn download_all(label: String, songs: Vec<Song>) {
    let mut downloaded = 0;
    let mut existing = 0;
    let mut failed = 0;
    let mut first_error = None;

    for song in songs {
        let download = { ncm_client.lock().await.download_song(song) };
        match download.await {
            Ok(DownloadResult::Downloaded(_)) => downloaded += 1,
            Ok(DownloadResult::AlreadyExists(_)) => existing += 1,
            Err(err) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
            },
        }
    }

    let mut message = format!("{}下载完成：成功 {}，已存在 {}，失败 {}", label, downloaded, existing, failed);
    if let Some(err) = first_error {
        message.push_str(&format!("；首个错误：{}", err));
    }
    send_download_message(message).await;
}

async fn send_download_message(message: String) {
    let mut command_queue_guard = command_queue.lock().await;
    command_queue_guard.push_back(Command::DownloadFinished(message));
    command_queue_guard.push_back(Command::RefreshPlaylist);
}
