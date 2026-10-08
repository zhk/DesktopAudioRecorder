//! 桌面音频录制（Windows）—— Rust 版，等价于仓库根目录的 C++ 实现。
//!
//! 同时录制：播放设备声音（WASAPI Loopback）+ 麦克风输入，混音后编码为 MP3。
//!
//! # 模块划分与 C++ 文件的对应关系
//!
//! | C++ | Rust |
//! |-----|------|
//! | `src/main.cpp`           | [`app`] + `src/main.rs` |
//! | `src/wasapi_capture.*`   | [`wasapi_capture`] |
//! | `src/audio_mixer.*`      | [`audio_mixer`] |
//! | `src/echo_canceller.*`   | [`echo_canceller`] |
//! | `src/mp3_writer.*`       | [`mp3_writer`] |
//! | `src/teams_watcher.*`    | [`teams_watcher`] |
//! | 系统头文件               | [`ffi`]（手写 FFI，无第三方依赖） |
//!
//! 二进制入口在 `src/main.rs`（`#![windows_subsystem = "windows"]`），
//! 其余全部逻辑都在本库中，便于单独测试（见 `examples/smoke.rs`）。

// 手写 FFI 难免要与 C 的命名风格保持一致，这里集中放开相关 lint。
#![allow(clippy::missing_safety_doc)]

pub mod app;
pub mod audio_mixer;
pub mod echo_canceller;
pub mod ffi;
pub mod mp3_writer;
pub mod teams_watcher;
pub mod util;
pub mod wasapi_capture;
