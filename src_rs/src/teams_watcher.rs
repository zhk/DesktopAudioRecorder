//! 会议检测：Teams、微信、钉钉、腾讯会议桌面版
//! 只有在通话/会议中才会在活跃播放设备上保持活跃的音频会话。
//! 会议窗口最小化、隐藏均不影响检测（不依赖窗口标题）。
//!
//! 与 C++ 版 `src/teams_watcher.{h,cpp}` 一一对应。

use core::ptr::null_mut;

use crate::ffi::system::{
    CloseHandle, CoCreateInstance, OpenProcess, QueryFullProcessImageNameW, MAX_PATH,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use crate::ffi::wasapi::{
    eRender, AudioSessionStateActive, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice, IMMDeviceCollection,
    IMMDeviceEnumerator, CLSID_MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use crate::ffi::{failed, succeeded, Com, ComPtr, CLSCTX_ALL, FALSE};
use crate::util::from_wide;

pub struct TeamsWatcher;

fn is_meeting_process(pid: u32, app: &mut String) -> bool {
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

        if name.eq_ignore_ascii_case("teams.exe") || name.eq_ignore_ascii_case("ms-teams.exe") {
            app.clear();
            app.push_str("Teams");
            return true;
        }
        if name.eq_ignore_ascii_case("wechat.exe") || name.eq_ignore_ascii_case("weixin.exe") {
            app.clear();
            app.push_str("WeChat");
            return true;
        }
        if name.eq_ignore_ascii_case("dingtalk.exe")
            || name.eq_ignore_ascii_case("dingtalklite.exe")
            || name.eq_ignore_ascii_case("DingTalk.exe")
        {
            app.clear();
            app.push_str("DingTalk");
            return true;
        }
        if name.eq_ignore_ascii_case("wemeet.exe")
            || name.eq_ignore_ascii_case("wemeetapp.exe")
            || name.eq_ignore_ascii_case("wemeetlauncher.exe")
        {
            app.clear();
            app.push_str("TencentMeeting");
            return true;
        }
        false
    }
}

impl TeamsWatcher {
    pub fn meeting_active(app: &mut String) -> bool {
        app.clear();
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

            let mut collection = ComPtr::<IMMDeviceCollection>::null();
            if failed(enumerator.expect_ref().enum_audio_endpoints(
                eRender,
                DEVICE_STATE_ACTIVE,
                collection.as_out_ptr(),
            )) {
                return false;
            }

            let mut device_count: u32 = 0;
            if failed(collection.expect_ref().get_count(&mut device_count)) {
                return false;
            }

            for index in 0..device_count {
                let mut device = ComPtr::<IMMDevice>::null();
                if failed(collection.expect_ref().item(index, device.as_out_ptr())) {
                    continue;
                }

                let mut manager = ComPtr::<IAudioSessionManager2>::null();
                if failed(device.expect_ref().activate(
                    &IAudioSessionManager2::IID,
                    CLSCTX_ALL,
                    null_mut(),
                    manager.as_void_out_ptr(),
                )) {
                    continue;
                }

                let mut sessions = ComPtr::<IAudioSessionEnumerator>::null();
                if failed(manager.expect_ref().get_session_enumerator(sessions.as_out_ptr())) {
                    continue;
                }

                let mut session_count: i32 = 0;
                if failed(sessions.expect_ref().get_count(&mut session_count)) {
                    continue;
                }

                for si in 0..session_count {
                    let mut control = ComPtr::<IAudioSessionControl>::null();
                    if failed(sessions.expect_ref().get_session(si, control.as_out_ptr())) {
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
                        && is_meeting_process(pid, app)
                    {
                        return true;
                    }
                }
            }

            false
        }
    }
}