//! 会议检测：Teams、微信、钉钉、腾讯会议桌面版
//! 只有在通话/会议中才会在活跃播放设备上保持活跃的音频会话。
//! 会议窗口最小化、隐藏均不影响检测（不依赖窗口标题）。
//!
//! 与 C++ 版 `src/teams_watcher.{h,cpp}` 一一对应。

use core::ptr::null_mut;

use crate::ffi::system::{
    CloseHandle, CoCreateInstance, CoTaskMemFree, OpenProcess, PropVariantClear,
    QueryFullProcessImageNameW, MAX_PATH, PROCESS_QUERY_LIMITED_INFORMATION,
};
use crate::ffi::wasapi::{
    eRender, AudioSessionStateActive, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice, IMMDeviceCollection,
    IMMDeviceEnumerator, IPropertyStore, CLSID_MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    PKEY_DEVICE_FRIENDLY_NAME, STGM_READ,
};
use crate::ffi::{failed, succeeded, Com, ComPtr, CLSCTX_ALL, FALSE, LPWSTR, PROPVARIANT, VT_LPWSTR};
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

/// 复制 NUL 结尾的 UTF-16 字符串（含结尾 NUL），与 `app.rs` 里设备 ID 的表示保持一致，
/// 这样两边拿到的 ID 才能直接比较。
///
/// # Safety
/// `p` 必须指向合法的 NUL 结尾 UTF-16 字符串。
unsafe fn wide_from_ptr(p: LPWSTR) -> Vec<u16> {
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::slice::from_raw_parts(p, len + 1).to_vec()
}

/// 端点的 `PKEY_Device_FriendlyName`（下拉框里显示的名字）。
unsafe fn device_name(device: &IMMDevice) -> String {
    let mut store = ComPtr::<IPropertyStore>::null();
    if failed(device.open_property_store(STGM_READ, store.as_out_ptr())) {
        return String::new();
    }
    let mut name = String::new();
    let mut pv = PROPVARIANT::zeroed();
    if succeeded(store.expect_ref().get_value(&PKEY_DEVICE_FRIENDLY_NAME, &mut pv))
        && pv.vt == VT_LPWSTR
        && !pv.data.pwsz_val.is_null()
    {
        name = from_wide(&wide_from_ptr(pv.data.pwsz_val));
    }
    PropVariantClear(&mut pv);
    name
}

impl TeamsWatcher {
    /// 只关心“是否在开会”的调用方用这个。
    pub fn meeting_active(app: &mut String) -> bool {
        let mut id = Vec::new();
        let mut name = String::new();
        Self::meeting_active_device(app, &mut id, &mut name)
    }

    /// `device_id` = 会议音频当前实际所在的播放设备 ID（含结尾 NUL），
    /// `device_name` = 该设备的友好名；检测不到会议时两者留空。
    pub fn meeting_active_device(app: &mut String, device_id: &mut Vec<u16>, device_name_out: &mut String) -> bool {
        app.clear();
        device_id.clear();
        device_name_out.clear();
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
                        // 这台设备就是会议音频当前所在的输出设备
                        let mut id_ptr: LPWSTR = null_mut();
                        if succeeded(device.expect_ref().get_id(&mut id_ptr)) && !id_ptr.is_null() {
                            *device_id = wide_from_ptr(id_ptr);
                            CoTaskMemFree(id_ptr as *mut core::ffi::c_void);
                        }
                        *device_name_out = device_name(device.expect_ref());
                        return true;
                    }
                }
            }

            false
        }
    }
}