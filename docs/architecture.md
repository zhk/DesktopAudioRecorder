# 系统架构

## 整体数据流

```
桌面 Loopback ─┐
               ├─→ AudioMixer ─→ Mp3Writer (Push 入队) ─→ MF Sink Writer ─→ .mp3 文件
麦克风 ────重采样──┘         ↑                ↑
               └─ 可选 AEC ──┘                独立编码线程
```

## 模块划分

### C++ 版 (`src/`)

| 模块 | 文件 | 职责 | 依赖 |
|------|------|------|------|
| 入口/窗口 | `main.cpp` | WinMain, WndProc, 系统托盘, UI 重建, WM_COMMAND/WM_SYSCOMMAND 分发 | recorder, localization, logger, device_notify |
| 国际化 | `localization.cpp/h` | `S()` 字符串查找, Load/SaveConfig, LoadLangFile | — |
| 日志 | `logger.cpp/h` | Timestamp, AppendLog (UTF-8+BOM), FormatDuration | — |
| 录制引擎 | `recorder.cpp/h` | StartRecording, StopRecording, OnWatchTick, ShowRecordingStatus, FillDeviceCombo, RefreshDevices | wasapi_capture, audio_mixer, mp3_writer, teams_watcher, localization, logger |
| WASAPI 采集 | `wasapi_capture.cpp/h` | InitLoopback, Run, 多声道降混, float→int16 转换 | localization (错误字符串) |
| MP3 编码 | `mp3_writer.cpp/h` | Open, Push, Run, WriteChunk, Close, Finalize | localization, Media Foundation |
| 混音器 | `audio_mixer.cpp/h` | Open, SetMicFormat, SetEchoCancellation, PushDesktop, PushMic, 重采样 | echo_canceller |
| 回声消除 | `echo_canceller.cpp/h` | Configure, PushReference, Process (NLMS + DTD) | — |
| Teams 检测 | `teams_watcher.cpp/h` | MeetingActive (IAudioSessionManager2 枚举 + 进程名匹配) | — |
| 设备通知 | `device_notify.h` | IMMNotificationClient, 设备变更 → WM_DEVICE_CHANGE | — |

### Rust 版 (`src_rs/src/`)

与 C++ 版模块一一对应，文件映射：

| C++ | Rust |
|-----|------|
| `main.cpp` | `app.rs` + `main.rs` |
| `localization.cpp/h` | `app.rs` (内联函数 `tr()`, `load_lang_file()`, `load_config()`, `save_config()`) |
| `logger.cpp/h` | `app.rs` (内联函数 `timestamp()`, `append_log()`, `format_duration()`) |
| `recorder.cpp/h` | `app.rs` (App struct 方法) |
| `wasapi_capture.cpp/h` | `wasapi_capture.rs` |
| `mp3_writer.cpp/h` | `mp3_writer.rs` |
| `audio_mixer.cpp/h` | `audio_mixer.rs` |
| `echo_canceller.cpp/h` | `echo_canceller.rs` |
| `teams_watcher.cpp/h` | `teams_watcher.rs` |
| Windows API | `ffi/` 目录 (system.rs, wasapi.rs, mf.rs, mod.rs) |

## 依赖方向

```
main.cpp
  ├── recorder.cpp ───────────────┐
  │     ├── wasapi_capture.cpp    │
  │     ├── mp3_writer.cpp        │
  │     ├── audio_mixer.cpp       │
  │     │     └── echo_canceller  │
  │     └── teams_watcher.cpp     │
  ├── localization.cpp            │
  ├── logger.cpp                  │
  └── device_notify.h             │
                                  │
注：无循环依赖                     │
main → recorder → wasapi/encoder/mixer → system APIs
main → localization (i18n strings)
recorder → localization (error strings)
recorder → logger (log writing)
```

## 线程模型

| 线程 | 职责 | 通信方式 |
|------|------|----------|
| UI 线程 | 窗口消息循环, WndProc, 控件操作 | Windows 消息队列 |
| 桌面采集线程 | WASAPI Loopback 事件循环 | `PostMessageW` (WM_RECORD_ERROR) → UI 线程 |
| 麦克风采集线程 | WASAPI Capture 事件循环 | `PostMessageW` → UI 线程 |
| MP3 编码线程 | 从队列取 PCM → WriteSample → SinkWriter | `std::mutex` + `std::condition_variable` |

采集线程与编码线程通过 `std::deque` 解耦——采集只管 Push，编码只管 Pop。

## 关键数据结构

### 全局状态 (C++)

```
g_hwnd, g_combo, g_comboMic, g_btnStart, g_btnRefresh,
g_btnOpenFolder, g_status, g_chkAec, g_chkAutoTeams, g_chkAutoStart,
g_recordCountText, g_recordCount

g_renderIds[], g_micIds[]     // 设备 ID 列表
g_capture, g_micCap           // 采集器对象
g_mixer, g_writer             // 混音器 + 编码器
g_recording, g_startTick, g_startTime, g_outPath
g_deviceName, g_micName
g_inTray, g_autoStarted, g_autoSuppress, g_inactivePolls
g_aecSaved, g_autoTeamsSaved, g_autoStartSaved  // checkbox 状态持久化
```

### App 状态 (Rust)

```rust
struct App {
    hwnd, combo, combo_mic, btn_start, btn_refresh,
    btn_open_folder, record_count_text, record_count,
    status, chk_aec, chk_auto_teams, chk_auto_start,
    aec_checked, auto_teams_checked, auto_start_checked,
    render_ids, mic_ids,
    session: Option<Session>,
    start_tick, start_time, out_path,
    device_name, mic_name, meeting_app,
    in_tray, auto_started, auto_suppress, inactive_polls,
}
```

## 消息定义

| 消息 ID | 值 | 含义 |
|---------|-----|------|
| IDM_TRAY_OPEN | 2001 | 托盘「打开」 |
| IDM_TRAY_EXIT | 2002 | 托盘「退出」 |
| IDM_SYS_ABOUT | 3001 | 系统菜单「关于」 |
| IDM_LANG_ZHCN | 3002 | 系统菜单「中文」 |
| IDM_LANG_ENUS | 3003 | 系统菜单「English」 |
| WM_RECORD_ERROR | WM_APP+1 | 录制线程致命错误 |
| WM_TRAYICON | WM_APP+2 | 托盘图标事件 |
| WM_DEVICE_CHANGE | WM_APP+4 | 音频设备变更通知 |

## 配置文件格式

`[exe名].ini`（exe 同目录，UTF-16LE with BOM，`\r\n` 行尾）:

```ini
[Language]
locale=zh-CN
[Options]
auto_teams=1
auto_start=0
```

## 语言文件格式

`lang/[locale].ini`（UTF-16LE with BOM）:

```ini
[UI]
key=value
[Status]
key=value
[About]
key=value
[Menu]
key=value
[Errors]
key=value
```

键值中 `\n` 会被替换为 `\r\n` 用于多行显示。