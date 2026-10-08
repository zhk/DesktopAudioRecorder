//! Media Foundation 的 FFI 声明（MP3 编码用的 Sink Writer）。
//!
//! 对应头文件：`mfobjects.h`（`IMFAttributes` / `IMFMediaType` /
//! `IMFMediaBuffer` / `IMFSample`）、`mfreadwrite.h`（`IMFSinkWriter`）、
//! `mfapi.h`（`MFStartup` / `MFCreate*` 与 `MF_MT_*` 属性键）。
//!
//! 所有 GUID 都是照 SDK 头文件抄下来的常量，因此不需要链接 `mfuuid`。

use super::*;
use core::ffi::c_void;

// `#[macro_export]` 会把宏放到 crate 根，因此子模块里要显式引入。
use crate::{com_interface, com_interface_ext};

type VoidPtr = *mut c_void;

// ---------------------------------------------------------------------------
// 常量与属性键
// ---------------------------------------------------------------------------

/// `MF_VERSION` = `MF_SDK_VERSION << 16 | MF_API_VERSION`
pub const MF_VERSION: u32 = 0x0002_0070;
/// `MFSTARTUP_FULL`
pub const MFSTARTUP_FULL: DWORD = 0;

/// `MFMediaType_Audio`
pub const MFMediaType_Audio: GUID =
    GUID::new(0x7364_7561, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
/// `MFAudioFormat_PCM`（= `DEFINE_MEDIATYPE_GUID(_, WAVE_FORMAT_PCM)`）
pub const MFAudioFormat_PCM: GUID =
    GUID::new(0x0000_0001, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
/// `MFAudioFormat_Float`
pub const MFAudioFormat_Float: GUID =
    GUID::new(0x0000_0003, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
/// `MFAudioFormat_MP3`（= `WAVE_FORMAT_MPEGLAYER3`，0x55）
pub const MFAudioFormat_MP3: GUID =
    GUID::new(0x0000_0055, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);

/// `MF_MT_MAJOR_TYPE`
pub const MF_MT_MAJOR_TYPE: GUID =
    GUID::new(0x48EB_A18E, 0xF8C9, 0x4687, [0xBF, 0x11, 0x0A, 0x74, 0xC9, 0xF9, 0x6A, 0x8F]);
/// `MF_MT_SUBTYPE`
pub const MF_MT_SUBTYPE: GUID =
    GUID::new(0xF7E3_4C9A, 0x42E8, 0x4714, [0xB7, 0x4B, 0xCB, 0x29, 0xD7, 0x2C, 0x35, 0xE5]);
/// `MF_MT_AUDIO_NUM_CHANNELS`
pub const MF_MT_AUDIO_NUM_CHANNELS: GUID =
    GUID::new(0x37E4_8BF5, 0x645E, 0x4C5B, [0x89, 0xDE, 0xAD, 0xA9, 0xE2, 0x9B, 0x69, 0x6A]);
/// `MF_MT_AUDIO_SAMPLES_PER_SECOND`
pub const MF_MT_AUDIO_SAMPLES_PER_SECOND: GUID =
    GUID::new(0x5FAE_EAE7, 0x0290, 0x4C31, [0x9E, 0x8A, 0xC5, 0x34, 0xF6, 0x8D, 0x9D, 0xBA]);
/// `MF_MT_AUDIO_AVG_BYTES_PER_SECOND`
pub const MF_MT_AUDIO_AVG_BYTES_PER_SECOND: GUID =
    GUID::new(0x1AAB_75C8, 0xCFEF, 0x451C, [0xAB, 0x95, 0xAC, 0x03, 0x4B, 0x8E, 0x17, 0x31]);
/// `MF_MT_AUDIO_BLOCK_ALIGNMENT`
pub const MF_MT_AUDIO_BLOCK_ALIGNMENT: GUID =
    GUID::new(0x322D_E230, 0x9EEB, 0x43BD, [0xAB, 0x7A, 0xFF, 0x41, 0x22, 0x51, 0x54, 0x1D]);
/// `MF_MT_AUDIO_BITS_PER_SAMPLE`
pub const MF_MT_AUDIO_BITS_PER_SAMPLE: GUID =
    GUID::new(0xF2DE_B57F, 0x40FA, 0x4764, [0xAA, 0x33, 0xED, 0x4F, 0x2D, 0x1F, 0xF6, 0x69]);
/// `MF_MT_AUDIO_VALID_BITS_PER_SAMPLE`
pub const MF_MT_AUDIO_VALID_BITS_PER_SAMPLE: GUID =
    GUID::new(0xD9BF_8D6A, 0x9530, 0x4B7C, [0x9D, 0xDF, 0xFF, 0x6F, 0xD5, 0x8B, 0xBD, 0x06]);
/// `MF_MT_ALL_SAMPLES_INDEPENDENT`
pub const MF_MT_ALL_SAMPLES_INDEPENDENT: GUID =
    GUID::new(0xC917_3739, 0x5E56, 0x461C, [0xB7, 0x13, 0x46, 0xFB, 0x99, 0x5C, 0xB9, 0x5F]);

// ---------------------------------------------------------------------------
// 接口
// ---------------------------------------------------------------------------

com_interface! {
    /// `IMFAttributes`（mfobjects.h）
    IMFAttributes, IMFAttributesVtbl,
    GUID::new(0x2CD2_D921, 0xC447, 0x44A7, [0xA1, 0x3C, 0x4A, 0xDA, 0xBF, 0xC2, 0x47, 0xE3]);
    fn get_item(guid_key: *const GUID, value: *mut PROPVARIANT) -> HRESULT;
    fn get_item_type(guid_key: *const GUID, out: *mut i32) -> HRESULT;
    fn compare_item(guid_key: *const GUID, value: *const PROPVARIANT, result: *mut BOOL) -> HRESULT;
    fn compare(theirs: *mut IMFAttributes, match_type: i32, result: *mut BOOL) -> HRESULT;
    fn get_uint32(guid_key: *const GUID, out: *mut u32) -> HRESULT;
    fn get_uint64(guid_key: *const GUID, out: *mut u64) -> HRESULT;
    fn get_double(guid_key: *const GUID, out: *mut f64) -> HRESULT;
    fn get_guid(guid_key: *const GUID, out: *mut GUID) -> HRESULT;
    fn get_string_length(guid_key: *const GUID, out: *mut u32) -> HRESULT;
    fn get_string(
        guid_key: *const GUID,
        value: LPWSTR,
        buf_size: u32,
        length: *mut u32
    ) -> HRESULT;
    fn get_allocated_string(
        guid_key: *const GUID,
        value: *mut LPWSTR,
        length: *mut u32
    ) -> HRESULT;
    fn get_blob_size(guid_key: *const GUID, out: *mut u32) -> HRESULT;
    fn get_blob(
        guid_key: *const GUID,
        buf: *mut u8,
        buf_size: u32,
        blob_size: *mut u32
    ) -> HRESULT;
    fn get_allocated_blob(guid_key: *const GUID, buf: *mut *mut u8, size: *mut u32) -> HRESULT;
    fn get_unknown(guid_key: *const GUID, iid: *const GUID, out: *mut VoidPtr) -> HRESULT;
    fn set_item(guid_key: *const GUID, value: *const PROPVARIANT) -> HRESULT;
    fn delete_item(guid_key: *const GUID) -> HRESULT;
    fn delete_all_items() -> HRESULT;
    fn set_uint32(guid_key: *const GUID, value: u32) -> HRESULT;
    fn set_uint64(guid_key: *const GUID, value: u64) -> HRESULT;
    fn set_double(guid_key: *const GUID, value: f64) -> HRESULT;
    fn set_guid(guid_key: *const GUID, value: *const GUID) -> HRESULT;
    fn set_string(guid_key: *const GUID, value: LPCWSTR) -> HRESULT;
    fn set_blob(guid_key: *const GUID, buf: *const u8, size: u32) -> HRESULT;
    fn set_unknown(guid_key: *const GUID, unknown: VoidPtr) -> HRESULT;
    fn lock_store() -> HRESULT;
    fn unlock_store() -> HRESULT;
    fn get_count(out: *mut u32) -> HRESULT;
    fn get_item_by_index(index: u32, key: *mut GUID, value: *mut PROPVARIANT) -> HRESULT;
    fn copy_all_items(dest: *mut IMFAttributes) -> HRESULT;
}

com_interface_ext! {
    /// `IMFMediaType`（mfobjects.h，继承 `IMFAttributes`）
    IMFMediaType, IMFMediaTypeVtbl,
    GUID::new(0x44AE_0FA8, 0xEA31, 0x4109, [0x8D, 0x2E, 0x4C, 0xAE, 0x49, 0x97, 0xC5, 0x55]),
    IMFAttributesVtbl;
    inherited {
        fn set_uint32(guid_key: *const GUID, value: u32) -> HRESULT;
        fn set_guid(guid_key: *const GUID, value: *const GUID) -> HRESULT;
    }
    own {
        fn get_major_type(out: *mut GUID) -> HRESULT;
        fn is_compressed_format(out: *mut BOOL) -> HRESULT;
        fn is_equal(media_type: *mut IMFMediaType, result: *mut BOOL) -> HRESULT;
        fn get_representation(guid_representation: *const GUID, out: *mut *mut PROPVARIANT)
            -> HRESULT;
        fn free_representation(guid_representation: *const GUID, value: *mut PROPVARIANT)
            -> HRESULT;
    }
}

com_interface! {
    /// `IMFMediaBuffer`（mfobjects.h）
    IMFMediaBuffer, IMFMediaBufferVtbl,
    GUID::new(0x045F_A593, 0x8799, 0x42B8, [0xBC, 0x8D, 0x89, 0x68, 0xC6, 0x45, 0x35, 0x07]);
    fn lock(buf: *mut *mut u8, max_length: *mut DWORD, current_length: *mut DWORD) -> HRESULT;
    fn unlock() -> HRESULT;
    fn get_current_length(out: *mut DWORD) -> HRESULT;
    fn set_current_length(length: DWORD) -> HRESULT;
    fn get_max_length(out: *mut DWORD) -> HRESULT;
}

com_interface_ext! {
    /// `IMFSample`（mfobjects.h，继承 `IMFAttributes`）
    IMFSample, IMFSampleVtbl,
    GUID::new(0xC40A_00F2, 0xB93A, 0x4D80, [0xAE, 0x8C, 0x5A, 0x1C, 0x63, 0x4F, 0x58, 0xE4]),
    IMFAttributesVtbl;
    inherited {}
    own {
        fn get_sample_flags(out: *mut DWORD) -> HRESULT;
        fn set_sample_flags(flags: DWORD) -> HRESULT;
        fn get_sample_time(out: *mut LONGLONG) -> HRESULT;
        fn set_sample_time(time: LONGLONG) -> HRESULT;
        fn get_sample_duration(out: *mut LONGLONG) -> HRESULT;
        fn set_sample_duration(duration: LONGLONG) -> HRESULT;
        fn get_buffer_count(out: *mut DWORD) -> HRESULT;
        fn get_buffer_by_index(index: DWORD, out: *mut *mut IMFMediaBuffer) -> HRESULT;
        fn convert_to_contiguous_buffer(out: *mut *mut IMFMediaBuffer) -> HRESULT;
        fn add_buffer(buffer: *mut IMFMediaBuffer) -> HRESULT;
        fn remove_buffer_by_index(index: DWORD) -> HRESULT;
        fn remove_all_buffers() -> HRESULT;
        fn get_total_length(out: *mut DWORD) -> HRESULT;
        fn copy_to_buffer(buffer: *mut IMFMediaBuffer) -> HRESULT;
    }
}

com_interface! {
    /// `IMFSinkWriter`（mfreadwrite.h）
    IMFSinkWriter, IMFSinkWriterVtbl,
    GUID::new(0x3137_F1CD, 0xFE5E, 0x4805, [0xA5, 0xD8, 0xFB, 0x47, 0x74, 0x48, 0xCB, 0x3D]);
    fn add_stream(target_media_type: *mut IMFMediaType, out: *mut DWORD) -> HRESULT;
    fn set_input_media_type(
        stream_index: DWORD,
        input_media_type: *mut IMFMediaType,
        encoding_parameters: VoidPtr
    ) -> HRESULT;
    fn begin_writing() -> HRESULT;
    fn write_sample(stream_index: DWORD, sample: *mut IMFSample) -> HRESULT;
    fn send_stream_tick(stream_index: DWORD, timestamp: LONGLONG) -> HRESULT;
    fn place_marker(stream_index: DWORD, context: VoidPtr) -> HRESULT;
    fn notify_end_of_segment(stream_index: DWORD) -> HRESULT;
    fn flush(stream_index: DWORD) -> HRESULT;
    fn finalize() -> HRESULT;
    fn get_service_for_stream(
        stream_index: DWORD,
        service: *const GUID,
        iid: *const GUID,
        out: *mut VoidPtr
    ) -> HRESULT;
    // 参数类型是 MF_SINK_WRITER_STATISTICS*；本项目不使用统计信息，用 void* 占位不影响 ABI。
    fn get_statistics(stream_index: DWORD, stats: VoidPtr) -> HRESULT;
}

// ---------------------------------------------------------------------------
// 外部函数
// ---------------------------------------------------------------------------

#[link(name = "mfplat")]
unsafe extern "system" {
    pub fn MFStartup(Version: u32, dwFlags: DWORD) -> HRESULT;
    pub fn MFShutdown() -> HRESULT;
    pub fn MFCreateMediaType(out: *mut *mut IMFMediaType) -> HRESULT;
    pub fn MFCreateMemoryBuffer(max_length: DWORD, out: *mut *mut IMFMediaBuffer) -> HRESULT;
    pub fn MFCreateSample(out: *mut *mut IMFSample) -> HRESULT;
}

#[link(name = "mfreadwrite")]
unsafe extern "system" {
    /// `pByteStream` 是 `IMFByteStream*`、`pAttributes` 是 `IMFAttributes*`；
    /// 本项目两个都传 `null`，因此用 `void*` 声明（ABI 完全一致）。
    pub fn MFCreateSinkWriterFromURL(
        output_url: LPCWSTR,
        byte_stream: VoidPtr,
        attributes: VoidPtr,
        out: *mut *mut IMFSinkWriter,
    ) -> HRESULT;
}
