cmake_minimum_required(VERSION 3.24)
project(desktop-audio-recorder VERSION 1.4.0 LANGUAGES CXX)

# 独立的桌面音频录制小工具：
#   WASAPI Loopback 采集 -> PCM16 -> Media Foundation MP3 编码
# 不依赖 libobs / OBS 源码树，直接用系统组件。

add_executable(desktop-audio-recorder WIN32
    src/main.cpp
    src/localization.cpp
    src/localization.h
    src/logger.cpp
    src/logger.h
    src/recorder.cpp
    src/recorder.h
    src/wasapi_capture.cpp
    src/wasapi_capture.h
    src/mp3_writer.cpp
    src/mp3_writer.h
    src/audio_mixer.cpp
    src/audio_mixer.h
    src/echo_canceller.cpp
    src/echo_canceller.h
    src/device_notify.h
    src/teams_watcher.cpp
    src/teams_watcher.h
    src/resource.h
    src/app.rc
)

target_compile_features(desktop-audio-recorder PRIVATE cxx_std_17)
# 静态链接 CRT（/MT），使 exe 不依赖 VC++ Redist，拷到任意 Win8+ 机器即可运行
set_property(TARGET desktop-audio-recorder PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
# 源文件为 UTF-8 且含中文字符串，/utf-8 让 MSVC 不按系统 ANSI 代码页误读
target_compile_options(desktop-audio-recorder PRIVATE /utf-8)
target_compile_definitions(desktop-audio-recorder PRIVATE
    UNICODE
    _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    WINVER=0x0602          # Windows 8+（内置 MP3 编码器 MFT）
    _WIN32_WINNT=0x0602
)
target_link_libraries(desktop-audio-recorder PRIVATE
    ole32
    mfplat
    mfreadwrite
    mfuuid
    avrt
    comctl32
)
