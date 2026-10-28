# HANDOFF — 项目交接

## 项目信息

- **项目名**: 桌面音频录制 (Desktop Audio Recorder)
- **一句话**: Windows 桌面音频录制，WASAPI Loopback + 麦克风混音 → MP3
- **技术栈**: C++17/CMake + Rust/Cargo，Win32 API，零第三方依赖
- **当前版本**: v1.3 (C++), v1.0.0 (Rust)
- **最后更新**: 2026-10（设备变更通知、开机自启、界面重排）

## 关键联系人

Zhang Kun(zhk.tiger@gmail.com)

## 环境信息

<待补充：开发机配置、构建工具链版本、目标 Windows 版本>

## 快速上手

1. 阅读 `README.md`（项目地图）
2. 阅读 `docs/architecture.md`（模块划分与数据流）
3. C++ 构建: `cd build && cmake --build . --config Release`
4. Rust 构建: `cd src_rs && cargo build --release`
5. 运行: exe 与 `lang/` 目录放同一目录下，双击即可

## 关键文件位置

| 想看什么 | 去这里 |
|----------|--------|
| 项目全局 | `README.md` |
| AI 工作协议 | `AGENTS.md` |
| 系统架构 | `docs/architecture.md` |
| 架构决策 | `docs/adr/0001-initial-architecture.md` |
| 回声消除详解 | `docs/aec.md` |
| 已知坑位 | `memory/pitfalls.md` |
| 当前进度 | `progress.md` |
| 主窗口代码 | `src/main.cpp` (C++), `src_rs/src/app.rs` (Rust) |
| 录制核心逻辑 | `src/recorder.cpp` |
| 音频采集 | `src/wasapi_capture.cpp` |
| MP3 编码 | `src/mp3_writer.cpp` |
| 混音器 | `src/audio_mixer.cpp` |
| 回声消除 | `src/echo_canceller.cpp` |
| 会议检测 | `src/teams_watcher.cpp` |
| 多语言 | `src/localization.cpp` |
| 日志 | `src/logger.cpp` |

## 当前待办

- [ ] Rust 版版本号更新
- [ ] 日志旧文件清理
- [ ] 退出确认

详见 `progress.md`

## 已知坑位

参见 `memory/pitfalls.md` — 包含构建工具链、GUI、音频、会议检测等方面的已知问题及解决方法。

## 历史会话

参见 `ai-sessions/INDEX.md` 和 `.workbuddy/memory/` 目录。

---

## 续接提示词

> 我已接手「桌面音频录制」项目（C++/Rust 双版本，v1.3）。请先阅读
> AGENTS.md、README.md、docs/architecture.md、progress.md、memory/pitfalls.md，
> 然后告诉我你对当前项目状态的理解，以及你建议的下一步工作。