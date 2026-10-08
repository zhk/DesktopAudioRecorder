//! 桌面音频录制（Windows）—— 二进制入口。
//!
//! 真正的逻辑都在 `desktop_audio_recorder` 库里（见 `src/app.rs`），
//! 这里只做两件事：把自己标记为 GUI 子系统（不弹控制台），以及启动消息循环。
//! 对应 C++ 版的 `wWinMain`。

// 与 CMakeLists.txt 里的 `add_executable(... WIN32 ...)` 等价
#![windows_subsystem = "windows"]

fn main() {
    let exit_code = desktop_audio_recorder::app::run();
    std::process::exit(exit_code);
}
