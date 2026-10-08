//! Core Audio（WASAPI）与 MMDevice API 的 FFI 声明。
//!
//! 对应头文件：`mmdeviceapi.h`、`audioclient.h`、`audiopolicy.h`、
//! `propsys.h`（`IPropertyStore`）、`mmreg.h`（`WAVEFORMATEX`）、
//! `functiondiscoverykeys_devpkey.h`（`PKEY_Device_FriendlyName`）。
//!
//! 所有 `DEFINE_GUID` 的值都是照着 SDK 头文件抄下来的，而不是链接 uuid.lib，
//! 因此不需要 `mfuuid` / `uuid` 之类的导入库。

use super::*;
use core::ffi::c_void;

// `#[macro_export]` 会把宏放到 crate 根，因此子模块里要显式引入。
use crate::com_interface;

type VoidPtr = *mut c_void;
type LPCGUID = *const GUID;

// ---------------------------------------------------------------------------
// 结构体
// ---------------------------------------------------------------------------

/// `WAVEFORMATEX`（mmreg.h 在 `pshpack1.h` 区间内声明，故为 1 字节对齐）。
///
/// 必须保持 18 字节：`WAVEFORMATEXTENSIBLE` 的布局依赖它。
#[repr(C, packed(1))]
#[derive(Copy, Clone)]
pub struct WAVEFORMATEX {
    pub wFormatTag: u16,
    pub nChannels: u16,
    pub nSamplesPerSec: u32,
    pub nAvgBytesPerSec: u32,
    pub nBlockAlign: u16,
    pub wBitsPerSample: u16,
    pub cbSize: u16,
}

/// `WAVEFORMATEXTENSIBLE`（= 40 字节）。
#[repr(C, packed(1))]
#[derive(Copy, Clone)]
pub struct WAVEFORMATEXTENSIBLE {
    pub Format: WAVEFORMATEX,
    pub wValidBitsPerSample: u16,
    pub dwChannelMask: u32,
    pub SubFormat: GUID,
}

const _: () = assert!(core::mem::size_of::<WAVEFORMATEX>() == 18);
const _: () = assert!(core::mem::size_of::<WAVEFORMATEXTENSIBLE>() == 40);

// ---------------------------------------------------------------------------
// 枚举值与常量
// ---------------------------------------------------------------------------

/// `EDataFlow`
pub type EDataFlow = i32;
/// 播放（渲染）设备
pub const eRender: EDataFlow = 0;
/// 录音（采集）设备
pub const eCapture: EDataFlow = 1;

/// `ERole`
pub type ERole = i32;
pub const eConsole: ERole = 0;

/// `AudioSessionState`
pub type AudioSessionState = i32;
/// 会话正在播放音频（≈ 会议进行中）
pub const AudioSessionStateActive: AudioSessionState = 1;

/// `DEVICE_STATE_ACTIVE`
pub const DEVICE_STATE_ACTIVE: DWORD = 0x0000_0001;
/// `STGM_READ`
pub const STGM_READ: DWORD = 0x0000_0000;

/// `AUDCLNT_SHAREMODE_SHARED`
pub const AUDCLNT_SHAREMODE_SHARED: i32 = 0;
/// `AUDCLNT_STREAMFLAGS_LOOPBACK`
pub const AUDCLNT_STREAMFLAGS_LOOPBACK: DWORD = 0x0002_0000;
/// `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`
pub const AUDCLNT_STREAMFLAGS_EVENTCALLBACK: DWORD = 0x0004_0000;
/// `AUDCLNT_BUFFERFLAGS_SILENT`
pub const AUDCLNT_BUFFERFLAGS_SILENT: DWORD = 0x0000_0002;
/// `AUDCLNT_S_BUFFER_EMPTY`
pub const AUDCLNT_S_BUFFER_EMPTY: HRESULT = 0x0889_0001;
/// `AUDCLNT_E_DEVICE_INVALIDATED`
pub const AUDCLNT_E_DEVICE_INVALIDATED: HRESULT = 0x8889_0004u32 as HRESULT;

/// `WAVE_FORMAT_PCM`
pub const WAVE_FORMAT_PCM: u16 = 0x0001;
/// `WAVE_FORMAT_IEEE_FLOAT`
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
/// `WAVE_FORMAT_EXTENSIBLE`
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// `CLSID_MMDeviceEnumerator`（mmdeviceapi.h: `DECLSPEC_UUID`）
pub const CLSID_MMDeviceEnumerator: GUID =
    GUID::new(0xBCDE0395, 0xE52F, 0x467C, [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E]);

/// `KSDATAFORMAT_SUBTYPE_PCM`（ksmedia.h）
pub const KSDATAFORMAT_SUBTYPE_PCM: GUID =
    GUID::new(0x0000_0001, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);

/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`（ksmedia.h）
pub const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::new(0x0000_0003, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);

/// `PKEY_Device_FriendlyName`（functiondiscoverykeys_devpkey.h）
pub const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::new(0xA45C_254E, 0xDF1C, 0x4EFD, [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0]),
    pid: 14,
};

// ---------------------------------------------------------------------------
// 接口
// ---------------------------------------------------------------------------

com_interface! {
    /// `IMMDevice`（mmdeviceapi.h）
    IMMDevice, IMMDeviceVtbl,
    GUID::new(0xD666063F, 0x1587, 0x4E43, [0x81, 0xF1, 0xB9, 0x48, 0xE8, 0x07, 0x36, 0x3F]);
    fn activate(
        iid: *const GUID,
        cls_ctx: DWORD,
        activation_params: *mut PROPVARIANT,
        out: *mut VoidPtr
    ) -> HRESULT;
    fn open_property_store(stgm_access: DWORD, out: *mut *mut IPropertyStore) -> HRESULT;
    fn get_id(out: *mut LPWSTR) -> HRESULT;
    fn get_state(out: *mut DWORD) -> HRESULT;
}

com_interface! {
    /// `IMMDeviceCollection`（mmdeviceapi.h）
    IMMDeviceCollection, IMMDeviceCollectionVtbl,
    GUID::new(0x0BD7A1BE, 0x7A1A, 0x44DB, [0x83, 0x97, 0xCC, 0x53, 0x92, 0x38, 0x7B, 0x5E]);
    fn get_count(out: *mut UINT) -> HRESULT;
    fn item(index: UINT, out: *mut *mut IMMDevice) -> HRESULT;
}

com_interface! {
    /// `IMMDeviceEnumerator`（mmdeviceapi.h）
    IMMDeviceEnumerator, IMMDeviceEnumeratorVtbl,
    GUID::new(0xA95664D2, 0x9614, 0x4F35, [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6]);
    fn enum_audio_endpoints(
        data_flow: EDataFlow,
        state_mask: DWORD,
        out: *mut *mut IMMDeviceCollection
    ) -> HRESULT;
    fn get_default_audio_endpoint(
        data_flow: EDataFlow,
        role: ERole,
        out: *mut *mut IMMDevice
    ) -> HRESULT;
    fn get_device(device_id: LPCWSTR, out: *mut *mut IMMDevice) -> HRESULT;
    // 参数类型是 IMMNotificationClient*；本项目从不注册回调，用 void* 占位不影响 ABI。
    fn register_endpoint_notification_callback(client: VoidPtr) -> HRESULT;
    fn unregister_endpoint_notification_callback(client: VoidPtr) -> HRESULT;
}

com_interface! {
    /// `IPropertyStore`（propsys.h）
    IPropertyStore, IPropertyStoreVtbl,
    GUID::new(0x886D8EEB, 0x8CF2, 0x4446, [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99]);
    fn get_count(out: *mut DWORD) -> HRESULT;
    fn get_at(index: DWORD, out: *mut PROPERTYKEY) -> HRESULT;
    fn get_value(key: *const PROPERTYKEY, out: *mut PROPVARIANT) -> HRESULT;
    fn set_value(key: *const PROPERTYKEY, value: *const PROPVARIANT) -> HRESULT;
    fn commit() -> HRESULT;
}

com_interface! {
    /// `IAudioClient`（audioclient.h）
    IAudioClient, IAudioClientVtbl,
    GUID::new(0x1CB9AD4C, 0xDBFA, 0x4C32, [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2]);
    fn initialize(
        share_mode: i32,
        stream_flags: DWORD,
        buffer_duration: REFERENCE_TIME,
        periodicity: REFERENCE_TIME,
        format: *const WAVEFORMATEX,
        audio_session_guid: LPCGUID
    ) -> HRESULT;
    fn get_buffer_size(out: *mut UINT32) -> HRESULT;
    fn get_stream_latency(out: *mut REFERENCE_TIME) -> HRESULT;
    fn get_current_padding(out: *mut UINT32) -> HRESULT;
    fn is_format_supported(
        share_mode: i32,
        format: *const WAVEFORMATEX,
        closest_match: *mut *mut WAVEFORMATEX
    ) -> HRESULT;
    fn get_mix_format(out: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn get_device_period(
        default_period: *mut REFERENCE_TIME,
        minimum_period: *mut REFERENCE_TIME
    ) -> HRESULT;
    fn start() -> HRESULT;
    fn stop() -> HRESULT;
    fn reset() -> HRESULT;
    fn set_event_handle(event: HANDLE) -> HRESULT;
    fn get_service(iid: *const GUID, out: *mut VoidPtr) -> HRESULT;
}

com_interface! {
    /// `IAudioCaptureClient`（audioclient.h）
    IAudioCaptureClient, IAudioCaptureClientVtbl,
    GUID::new(0xC8ADBD64, 0xE71E, 0x48A0, [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17]);
    fn get_buffer(
        data: *mut *mut u8,
        frames: *mut UINT32,
        flags: *mut DWORD,
        device_position: *mut UINT64,
        qpc_position: *mut UINT64
    ) -> HRESULT;
    fn release_buffer(frames: UINT32) -> HRESULT;
    fn get_next_packet_size(out: *mut UINT32) -> HRESULT;
}

com_interface! {
    /// `IAudioSessionManager` 的两个方法位于 `IAudioSessionManager2` vtable 的最前面。
    /// 这里把它们与派生方法一起平铺声明（顺序与 audiopolicy.h 一致）。
    IAudioSessionManager2, IAudioSessionManager2Vtbl,
    GUID::new(0x77AA99A0, 0x1BD6, 0x484F, [0x8B, 0xC7, 0x2C, 0x65, 0x4C, 0x9A, 0x9B, 0x6F]);
    fn get_audio_session_control(
        session_guid: LPCGUID,
        stream_flags: DWORD,
        out: *mut VoidPtr
    ) -> HRESULT;
    fn get_simple_audio_volume(
        session_guid: LPCGUID,
        stream_flags: DWORD,
        out: *mut VoidPtr
    ) -> HRESULT;
    fn get_session_enumerator(out: *mut *mut IAudioSessionEnumerator) -> HRESULT;
    // 参数类型是 IAudioSessionNotification* / IAudioVolumeDuckNotification*，
    // 本项目从不注册回调，用 void* 占位不影响 ABI。
    fn register_session_notification(notification: VoidPtr) -> HRESULT;
    fn unregister_session_notification(notification: VoidPtr) -> HRESULT;
    fn register_duck_notification(notification: VoidPtr) -> HRESULT;
    fn unregister_duck_notification(notification: VoidPtr) -> HRESULT;
}

com_interface! {
    /// `IAudioSessionEnumerator`（audiopolicy.h）
    IAudioSessionEnumerator, IAudioSessionEnumeratorVtbl,
    GUID::new(0xE2F5BB11, 0x0570, 0x40CA, [0xAC, 0xDD, 0x3A, 0xA0, 0x12, 0x77, 0xDE, 0xE8]);
    fn get_count(out: *mut i32) -> HRESULT;
    fn get_session(index: i32, out: *mut *mut IAudioSessionControl) -> HRESULT;
}

com_interface! {
    /// `IAudioSessionControl`（audiopolicy.h）
    IAudioSessionControl, IAudioSessionControlVtbl,
    GUID::new(0xF4B1A599, 0x7266, 0x4319, [0xA8, 0xCA, 0xE7, 0x0A, 0xCB, 0x11, 0xE8, 0xCD]);
    fn get_state(out: *mut AudioSessionState) -> HRESULT;
    fn get_display_name(out: *mut LPWSTR) -> HRESULT;
    fn set_display_name(value: LPCWSTR, event_context: LPCGUID) -> HRESULT;
    fn get_icon_path(out: *mut LPWSTR) -> HRESULT;
    fn set_icon_path(value: LPCWSTR, event_context: LPCGUID) -> HRESULT;
    fn get_grouping_param(out: *mut GUID) -> HRESULT;
    fn set_grouping_param(override_: LPCGUID, event_context: LPCGUID) -> HRESULT;
    fn register_audio_session_notification(notifications: VoidPtr) -> HRESULT;
    fn unregister_audio_session_notification(notifications: VoidPtr) -> HRESULT;
}

com_interface! {
    /// `IAudioSessionControl2`（audiopolicy.h，继承 IAudioSessionControl 的 9 个方法）
    IAudioSessionControl2, IAudioSessionControl2Vtbl,
    GUID::new(0xBFB7FF88, 0x7239, 0x4FC9, [0x8F, 0xA2, 0x07, 0xC9, 0x50, 0xBE, 0x9C, 0x6D]);
    fn get_state(out: *mut AudioSessionState) -> HRESULT;
    fn get_display_name(out: *mut LPWSTR) -> HRESULT;
    fn set_display_name(value: LPCWSTR, event_context: LPCGUID) -> HRESULT;
    fn get_icon_path(out: *mut LPWSTR) -> HRESULT;
    fn set_icon_path(value: LPCWSTR, event_context: LPCGUID) -> HRESULT;
    fn get_grouping_param(out: *mut GUID) -> HRESULT;
    fn set_grouping_param(override_: LPCGUID, event_context: LPCGUID) -> HRESULT;
    fn register_audio_session_notification(notifications: VoidPtr) -> HRESULT;
    fn unregister_audio_session_notification(notifications: VoidPtr) -> HRESULT;
    fn get_session_identifier(out: *mut LPWSTR) -> HRESULT;
    fn get_session_instance_identifier(out: *mut LPWSTR) -> HRESULT;
    fn get_process_id(out: *mut DWORD) -> HRESULT;
    fn is_system_sounds_session() -> HRESULT;
    fn set_ducking_preference(opt_out: BOOL) -> HRESULT;
}
