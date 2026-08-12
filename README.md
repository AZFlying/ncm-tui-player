# NCM-TUI-Player

![desktop-icon](./public/icon.png)

本项目是一款用 Rust 编写的网易云音乐终端界面播放器。

提供仿 `Vim` 式的命令和交互体验。

欢迎提 Issue 或 PR :)

## UI 展示

### 启动界面

![launch_screen](./doc/launch_screen.png)

### 主界面

![main_screen](./doc/main_screen.png)

### 主界面（全屏）

![main_screen_full](./doc/main_screen_full.png)

### 登录界面

二维码

![login_screen](./doc/login_screen.png)

## Features

### 登录
- [x] 扫码登录
- [x] Cookie 登录

### 播放 / 歌词
- [x] 音量设置
  - [x] “一键静音”
- [x] 播放模式
  - [x] 单曲播放
  - [x] 单曲循环播放
  - [x] 列表循环播放
  - [x] 随机播放
- [x] “一键开始播放”
- [x] 歌词滚动显示
- [x] 跳转到某句歌词对应的时间戳播放
- [ ] 播放记录计入网易云云端记录和听歌报告（上游接口目前疑似高危）

### 播放列表
- [x] 播放用户歌单（创建+收藏）
- [x] 在播放列表中跳转到当前播放的歌曲
- [x] 播放结束自动切歌时，播放列表光标跟随当前歌曲
- [x] 在播放列表中搜索歌曲名
  - [ ] 支持正则表达式
- [x] 创建 / 删除歌单（歌单界面按 `n` / `d`，或使用 `playlist create` / `playlist delete` 命令）
- [x] 收藏歌曲到指定歌单 / 从歌单移除（`collect` / `uncollect` 命令，候选列表实时过滤补全）

### 歌曲
- [ ] 全局搜索歌曲
- [x] 下载歌曲 / 歌单，并优先播放本应用下载的本地文件
- [ ] 歌曲操作
  - [x] 喜欢 / 取消喜欢（按 `l` 切换，或使用 `like` / `unlike` 命令）
  - [x] 收藏到自建歌单 / 取消收藏（`collect` / `uncollect` 命令）；歌单界面右侧可按 `d` 移除高亮歌曲
  - [ ] 查看所属专辑
  - [ ] 查看歌手主页

### 其他
- [x] 本地 api + 远程 api
- [ ] 适配系统媒体播放接口
  - [ ] MPRIS (Linux)
  - [ ] SMTC (Windows)
- [ ] 自定义Style
- [x] 设置页面（按 `9` 或 `screen 9` 进入，可修改 API 模式 / 播放音质 / 下载配置，立即生效）
- [ ] 用户数据缓存
- [ ] 打包分发
  - [x] Linux (rpm)
  - [x] Linux (deb)
  - [ ] Linux (flatpak)
  - [ ] MacOS
  - [ ] Windows

## 依赖和安装

本项目依赖上游的 [`neteasecloudmusicapi`](https://github.com/Binaryify/NeteaseCloudMusicApi) 项目（为nodejs程序），支持两种模式
- `local api` 模式，依赖程序部署在本地
  - 访问速度很快，没有账号安全隐患
  - 需要在本地部署依赖程序，占用较大存储空间（100MB左右）（此外还需要nodejs环境），运行时也需占用额外内存
- `remote api` 模式，依赖程序部署在服务器
  - 本地无需安装大量依赖，节省空间，快捷部署
  - 依赖程序部署在服务器，本项目提供了默认的公开服务器，但访问速度较慢（性能较弱）（**如果您有空闲服务器，也欢迎提供**）
  - 如果使用第三方部署的 remote api ，可能存在安全隐患

两种模式的切换（`use_remote_api` / `remote_api_url`）可在设置界面（按 `9`）修改，立即生效。

### 对于 local api 模式

依赖如下：
- [Gstreamer](https://gstreamer.freedesktop.org/download)
- [nodejs 14+](https://nodejs.org/)
- [netease-cloud-music-api](./bin/neteasecloudmusicapi.zip)

#### 1. 自行准备 nodejs 和 npm 环境

nodejs 版本 >= 14

#### 2. 解压 netease-cloud-music-api

请将项目 `bin` 目录下的 `neteasecloudmusicapi.zip` 解压到对应操作系统的指定路径:

|   OS    |                            解压到                             |
|:-------:|:----------------------------------------------------------:|
|  Linux  |         /home/`$USER`/.local/share/ncm-tui-player/         |
|  MacOS  | /Users/`$USER`/Library/Application Support/ncm-tui-player/ |
| Windows |   C:\\Users\\`$USER`\\AppData\\Roaming\\ncm-tui-player\\   |

解压后的文件树如下：

```
ncm-tui-player
└── neteasecloudmusicapi
    ├── app.js
    ├── CHANGELOG.MD
    ├── data
    ├── ...
    └── yarn.lock
```

#### 3. 安装 netease-cloud-music-api

切换到步骤 `2.` 中的目录，执行

```shell
cd neteasecloudmusicapi
npm install
```

#### 4. 安装 Gstreamer

本项目至少需要 `gstreamer-base` `gstreamer-good` 和 `gstreamer-bad` 组件，
请参考 [官方文档](https://gstreamer.freedesktop.org/documentation/installing/index.html?gi-language=c) 自行安装对应系统的版本。

### 对于 remote api 模式

依赖如下：
- [Gstreamer](https://gstreamer.freedesktop.org/download)

Gstreamer 的安装与 `local api` 模式下相同。

## 下载与本地播放

命令行模式支持：

- `download song`：下载当前高亮歌曲（主界面或歌单界面右侧）。
- `download playlist`：下载当前播放列表，或歌单界面左侧高亮歌单。

下载在后台顺序执行。播放器会优先查找本应用下载的同 ID 歌曲，没有本地文件时再在线播放。
下载歌曲时会同时下载歌词文件（默认命名 `{曲名}-Lyric.lrc`），歌词下载失败不影响歌曲本身。
已下载的歌曲在播放列表曲名前显示 `↓` 标识（与喜欢的 `♥` 叠加时显示为 `♥↓`）。

## 歌单管理

命令行模式支持：

- `collect <歌单名>`：收藏光标所在歌曲到指定自建歌单。
- `uncollect <歌单名>`：从指定自建歌单移除光标所在歌曲。
- `playlist create <名称>`：创建歌单（默认私有）。
- `playlist delete <名称>`：删除自建歌单（需按 `y` 确认）。
- `remove`：将播放列表中光标所在歌曲从当前播放列表移除（需按 `y` 确认）；主界面按 `d` 等效。

输入 `collect` / `uncollect` / `playlist delete` 后自动展开歌单候选列表：输入任意字符实时过滤（子串匹配、不区分大小写），`↑`/`↓`（或 `Tab`/`Shift+Tab`）选择，`Enter` 填入，`Esc` 关闭。列表仅列出自建歌单，歌单名可含空格。

歌单界面快捷键：左侧面板按 `n` 新建歌单、按 `d` 删除高亮歌单（收藏的歌单不可删除）；右侧歌曲列表按 `d` 从当前浏览的歌单移除高亮歌曲（需按 `y` 确认）。主界面按 `n` 会预填 `collect ` 命令并展开候选列表，可快速收藏光标所在歌曲；播放自建歌单时按 `d` 可将光标歌曲移出该歌单（需确认）。以上操作均只对自建歌单生效，且「我喜欢的音乐」歌单不参与收藏 / 移除。

下载配置位于配置目录的 `settings.json`（Linux 默认为 `~/.config/ncm-tui-player/settings.json`，旧版本数据目录下的文件会在启动时自动迁移过去）。推荐按 `9` 打开设置界面直接修改（立即生效），也可手动编辑后重启：

```json
{
  "download_path": "/absolute/path/to/music",
  "download_quality": "jymaster",
  "play_quality": "hires",
  "download_file_name_pattern": "{name}-{singer}-{album}-{quality}-{id}",
  "download_lyric_name_pattern": "{name}-Lyric"
}
```

`download_path` 建议使用绝对路径。`download_quality` 支持：`standard`、`higher`、`exhigh`、`lossless`、`hires`、`jyeffect`、`sky`、`dolby`、`jymaster`，该字段只控制下载音质。在线播放音质由 `play_quality` 控制，支持同一组取值，默认 `hires`（部分音质需要黑胶 VIP，否则会按账号权限回落）。

`download_file_name_pattern` 控制下载文件命名，可用占位符：`{name}`（曲名）、`{singer}`（作者）、`{album}`（专辑）、`{quality}`（请求音质）、`{id}`（歌曲 ID）。为保证本地优先播放与重复下载检测正常工作，命名中需保留 `{id}`（置于开头或结尾）与 `{quality}`（两侧以 `-` 分隔）。

`download_lyric_name_pattern` 控制歌词文件命名（扩展名固定为 `.lrc`），支持同一套占位符。默认 `{name}-Lyric` 只含曲名，同名歌曲会共用先下载的歌词文件；如需区分可自行加入 `{id}` 等占位符。

## 运行说明

运行时需要将 `stderr` 输出重定向。参考 `./bin/ncm-tui-player.sh` 脚本。

## 编译

除了使用本项目提供的打包，也欢迎您选择在本地自行编译。

### 1. Windows 下安装 Gstreamer 并编译本项目

根据 [Gstreamer 官方文档](https://gstreamer.freedesktop.org/documentation/installing/on-windows.html?gi-language=c) ，
需要安装 Gstreamer 的开发环境和运行时环境，此处注意选择的版本应与你的 rust 工具链使用的编译环境相同：
- 如果你的 rust 工具链使用了 `MSVC` ，则需要下载 Gstreamer 的 `MSVC 64-bit runtime installer` 和 `MSVC 64-bit development installer`
- 如果你的 rust 工具链使用了 `MinGW` ，则需要下载 Gstreamer 的 `MinGW 64-bit runtime installer` 和 `MinGW 64-bit development installer`

安装时选择 `Typical` 配置即可。

安装后需要设置一些环境变量，以下演示中均以 Gstreamer 选择 `MSVC 64-bit` 版本且安装在 `C:\gstreamer` 路径的情况为例：
- `Path` 变量中加入 Gstreamer 的 bin 目录 `C:\gstreamer\1.0\msvc_x86_64\bin`
- 新建 `GST_PLUGIN_PATH` 变量，值为 Gstreamer 的插件目录 `C:\gstreamer\1.0\msvc_x86_64\lib\gstreamer-1.0`

至此 Gstreamer 安装完毕。

在编译本项目前需要设置 `PKG_CONFIG_PATH` 环境变量（永久或临时均可）为：
`C:\gstreamer\1.0\msvc_x86_64\lib\pkgconfig;C:\gstreamer\1.0\msvc_x86_64\lib\gstreamer-1.0\pkgconfig` （包含2个目录）。

## 参考项目

https://gitlab.com/jcheatum/rmup

https://github.com/aome510/spotify-player

https://github.com/Rigellute/spotify-tui

https://github.com/sudipghimire533/ytui-music

https://github.com/tramhao/termusic