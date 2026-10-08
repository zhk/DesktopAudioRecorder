//! 小工具：Rust 字符串/路径与 Windows UTF-16 之间的转换。

use std::path::Path;

/// `&str` -> NUL 结尾的 UTF-16（可直接当 `LPCWSTR` 用）。
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// NUL 结尾的 UTF-16 -> `String`（遇到 NUL 即截断，非法序列按替换字符处理）。
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 路径 -> NUL 结尾的 UTF-16（用 `OsStr`，不经过 UTF-8，非 ASCII 路径也不会丢字符）。
pub fn path_to_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}
