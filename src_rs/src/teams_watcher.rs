//! Teams 会议检测：Teams（经典版 `Teams.exe` / 新版 `ms-teams.exe`）只有在
//! 通话/会议中才会在默认播放设备上保持活跃的音频会话。会议窗口最小化、
//! 隐藏均不影响检测（不依赖窗口标题）。
//!
//! 与 C++ 版 `src/teams_watcher.{h,cpp}` 一一对应。

use core::ptr::null_mut;

use crate::ffi::system::{
    CloseHandle, CoCreateInstance, OpenProcess, QueryFullProcessImageNameW, MAX_PATH,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use crate::ffi::wasapi::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    CLSID_MMDeviceEnumerator,
};
use crate::ffi::{failed, succeeded, Com, ComPtr, CLSCTX_ALL, FALSE};
use crate::util::from_wide;

pub struct TeamsWatcher;

impl TeamsWatcher {
    /// 当前是否存在 Teams 的活跃音频会话（≈ 正在会议中）。
    pub fn meeting_active() -> bool {
        // Safety: 只读查询；每个 COM 对象都用 ComPtr 管理生命周期，
        // 且在同一个线程内创建与释放。
        unsafe {
            let mut enumerator = ComPtr::<IMMDeviceEnumerator>::null();
            let hr = CoCreateInstance(
                &CLSID_MMDeviceEnumerator,
                null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::IID,
                enumerator.as_void_out_ptr(),
            );
            if failed(hr) {
                return false;
            }

            let mut device = ComPtr::<IMMDevice>::null();
            if failed(enumerator.expect_ref().get_default_audio_endpoint(
                eRender,
                eConsole,
                device.as_out_ptr(),
            )) {
                return false;
            }

            let mut manager = ComPtr::<IAudioSessionManager2>::null();
            if failed(device.expect_ref().activate(
                &IAudioSessionManager2::IID,
                CLSCTX_ALL,
                null_mut(),
                manager.as_void_out_ptr(),
            )) {
                return false;
            }

            let mut sessions = ComPtr::<IAudioSessionEnumerator>::null();
            if failed(manager.expect_ref().get_session_enumerator(sessions.as_out_ptr())) {
                return false;
            }

            let mut count: i32 = 0;
            if failed(sessions.expect_ref().get_count(&mut count)) {
                return false;
            }

            for index in 0..count {
                let mut control = ComPtr::<IAudioSessionControl>::null();
                if failed(sessions.expect_ref().get_session(index, control.as_out_ptr())) {
                    continue;
                }

                let mut state: i32 = 0;
                if !succeeded(control.expect_ref().get_state(&mut state))
                    || state != AudioSessionStateActive
                {
                    continue;
                }

                let Ok(control2) = control.query::<IAudioSessionControl2>() else {
                    continue;
                };
                let mut pid: u32 = 0;
                if succeeded(control2.expect_ref().get_process_id(&mut pid))
                    && pid != 0
                    && is_teams_process(pid)
                {
                    return true;
                }
            }

            false
        }
    }
}

/// 进程可执行文件名是否为 Teams（`Teams.exe` / `ms-teams.exe`，忽略大小写）。
fn is_teams_process(pid: u32) -> bool {
    // Safety: 只读查询进程映像名；句柄在返回前关闭。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if handle.is_null() {
            return false;
        }

        let mut buffer = [0u16; MAX_PATH];
        let mut size = MAX_PATH as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return false;
        }

        let path = from_wide(&buffer);
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        name.eq_ignore_ascii_case("teams.exe") || name.eq_ignore_ascii_case("ms-teams.exe")
    }
}
