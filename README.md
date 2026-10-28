# 桌面音频录制 (Desktop Audio Recorder)

Windows 桌面音频录制工具，WASAPI Loopback + 麦克风混音，MP3 编码存储，支持多会议 App 自动录
制。

## 版本

v1.3 (C++) / v1.0.0 (Rust)

## 一句话简介

录制系统播放设备输出的声音（会议中对方的声音），并可同时混入麦克风（自己的声音），保存为 MP3。
支持 Teams、微信、钉钉、腾讯会议的自动检测录制。

## 核心能力

- 桌面音频录制（WASAPI Loopback）
- 麦克风录制（可选，独立采集线程）
- 双路混音 + 线性插值重采样对齐
- NLMS 回声消除（AEC），带双讲检测
- Media Foundation 内置 MP3 编码器（192 kbps CBR）
- 多会议 App 自动检测录制（Teams / 微信 / 钉钉 / 腾讯会议）
- 会议 App 分目录存放录制文件
- 系统托盘运行
- 多语言界面（中文 / English），实时切换
- 单进程限制
- 录制日志（UTF-8, 按月切割）
- 开机自动启动（可选）
- 设备变更自动检测刷新

## 不做的范围

- 非 Windows 平台
- 非 MP3 格式输出
- GPU 加速编码
- 视频录制
- 远程网络录制
- 插件系统

## 快速开始

### 环境要求

- **C++ 版**: Windows 8+, Visual Studio 2022 (MSVC 14.44+), CMake 3.24+, Windows SDK 10.0.26100+
- **Rust 版**: Windows 8+, Rust 1.82+, MSVC toolchain

两个版本均不需要安装 VC++ Redist（静态链接 CRT）。Rust 版零第三方 crate 依赖。

### 构建与运行

**C++ (CMake):**

```bash
mkdir build && cd build
cmake .. -G "Visual Studio 17 2022" -A x64
cmake --build . --config Release
# 产物: build/Release/desktop-audio-recorder.exe
```

或使用根目录的批处理脚本（在沙箱环境 / 无法调用 vcvars 时）:

```bash
build_release.bat
```

**Rust (Cargo):**

```bash
cd src_rs
cargo build --release
# 产物: src_rs/target/release/desktop-audio-recorder.exe
```

### 运行

将 exe 和 `lang/` 目录（含 `zh-CN.ini`、`en-US.ini`）放在同一目录下运行即可。

首次运行会在 exe 同目录生成 `[exe名].ini` 配置文件（记录语言选择、checkbox 状态等）。

## 目录结构

| 目录/文件 | 内容 | 维护者 |
|-----------|------|--------|
| `src/` | C++ 版全部源码 | 主实现 |
| `src/main.cpp` | 入口、窗口过程、系统托盘、UI 重建 | |
| `src/localization.cpp/h` | `S()` 多语言字符串加载、配置读写 | |
| `src/logger.cpp/h` | 时间戳、UTF-8 日志写入、时长格式化 | |
| `src/recorder.cpp/h` | 开始/停止录制、设备枚举、混音、AEC 接线、Teams 看门狗 | |
| `src/wasapi_capture.cpp/h` | WASAPI Loopback/Capture 采集、多声道降混 | |
| `src/mp3_writer.cpp/h` | Media Foundation MP3 编码、独立编码线程 | |
| `src/audio_mixer.cpp/h` | 双路混音、麦克风重采样对齐 | |
| `src/echo_canceller.cpp/h` | NLMS-AEC + 双讲检测 | |
| `src/teams_watcher.cpp/h` | 音频会话枚举、会议 App 进程匹配 | |
| `src/device_notify.h` | `IMMNotificationClient` 设备变更通知 | |
| `src_rs/` | Rust 版全部源码（功能与 C++ 版等价） | 等价实现 |
| `src_rs/src/app.rs` | GUI 主逻辑 | |
| `src_rs/src/ffi/` | 手写 Win32/COM/MF FFI 声明 | |
| `lang/` | 多语言 ini 文件（UTF-16LE, zh-CN / en-US） | |
| `build/` | C++ CMake 构建目录（gitignore） | 自动生成 |
| `src_rs/target/` | Rust 构建目录（gitignore） | 自动生成 |
| `docs/` | 技术文档 | |
| `docs/aec.md` | 回声消除算法详解 | |
| `docs/architecture.md` | 系统架构全景 | |
| `docs/adr/` | 架构决策记录 | |
| `memory/` | 当前进度、坑位记录 | |
| `ai-sessions/` | AI 会话记录 | |
| `.workbuddy/` | 历史工作会话记录 | 遗留 |

## 我想知道 XX 该去哪看

| 问题 | 位置 |
|------|------|
| 如何开始录制？ | `src/recorder.cpp:StartRecording()` |
| 音频怎么抓取的？ | `src/wasapi_capture.cpp:InitLoopback()` |
| 回声消除怎么工作的？ | `docs/aec.md` |
| MP3 怎么编码的？ | `src/mp3_writer.cpp:Open()` |
| 会议怎么检测的？ | `src/teams_watcher.cpp:MeetingActive()` |
| 混音怎么做的？ | `src/audio_mixer.cpp` |
| UI 窗口怎么创建的？ | `src/main.cpp:WndProc()` / `RecreateControls()` |
| 多语言怎么加载的？ | `src/localization.cpp:LoadLangFile()` |
| 日志写在哪？ | exe 同目录 `[exe名]-YYYYMM.log` |
| 配置文件在哪？ | exe 同目录 `[exe名].ini` |
| 录音文件放哪？ | exe 同目录 / App 子目录（Teams/WeChat/DingTalk/TencentMeeting/） |

## 当前状态

- 阶段：功能完整，可日常使用
- 最近更新：v1.3 — 设备变更通知、开机自启、checkbox 状态持久化
- 已知问题：README.md 中的 "## 版本 v1.3" 注释已过，无

## Rust 版 Cargo.toml 版本号

`src_rs/Cargo.toml` 中 `version = "1.0.0"` 与实际版本不一致，需更新为 v1.3。

## 术语表

| 术语 | 含义 |
|------|------|
| WASAPI | Windows Audio Session API，低延迟音频采集 |
| Loopback | 录制播放设备输出（而非麦克风输入） |
| MF / Media Foundation | Windows 内置多媒体框架，提供 MP3 编码器 |
| AEC | Acoustic Echo Cancellation，回声消除 |
| NLMS | Normalized Least Mean Square，自适应滤波算法 |
| DTD | Double Talk Detection，双讲检测 |
| PCM16 | 16-bit linear PCM 音频格式 |
| CBR | Constant Bit Rate，固定码率 |
| Sink Writer | Media Foundation 的编码+写入组件 |
| COM | Component Object Model，Windows 组件模型 |
| Teams Watcher | 通过音频会话枚举检测会议 App 的模块 |