//! kernel32 / user32 / shell32 / comctl32 / ole32 / avrt 的 FFI 声明，
//! 以及 GUI 需要的结构体与常量。
//!
//! 结构体布局与 SDK 头文件一致（`#[repr(C)]`）；`WAVEFORMATEX` 这类在
//! `pshpack1.h` 区间内声明的结构体见 [`crate::ffi::wasapi`]。

use super::*;

// ---------------------------------------------------------------------------
// 结构体
// ---------------------------------------------------------------------------

/// `SYSTEMTIME`
#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

/// `POINT`
#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

/// `RECT`
#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `MSG`
#[repr(C)]
#[derive(Copy, Clone)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
}

/// 窗口过程类型。
pub type WNDPROC = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;

/// `WNDCLASSEXW`
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WNDCLASSEXW {
    pub cbSize: UINT,
    pub style: UINT,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: LPCWSTR,
    pub lpszClassName: LPCWSTR,
    pub hIconSm: HICON,
}

/// `INITCOMMONCONTROLSEX`
#[repr(C)]
#[derive(Copy, Clone)]
pub struct INITCOMMONCONTROLSEX {
    pub dwSize: DWORD,
    pub dwICC: DWORD,
}

/// `NOTIFYICONDATAW`（Vista+ 的完整布局）。
///
/// 字段顺序/偏移与 shellapi.h 一致：x64 上 `cbSize` 为 976 字节。
/// 结构体下方的断言会把布局错误变成编译错误，而不是运行期的托盘异常。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct NOTIFYICONDATAW {
    pub cbSize: DWORD,
    pub hWnd: HWND,
    pub uID: UINT,
    pub uFlags: UINT,
    pub uCallbackMessage: UINT,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: DWORD,
    pub dwStateMask: DWORD,
    pub szInfo: [u16; 256],
    /// 与 `uVersion` 共用同一块空间（`union { UINT uTimeout; UINT uVersion; }`）。
    pub uTimeoutOrVersion: UINT,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: DWORD,
    pub guidItem: GUID,
    pub hBalloonIcon: HICON,
}

const _: () = assert!(
    core::mem::size_of::<NOTIFYICONDATAW>() == if cfg!(target_pointer_width = "64") { 976 } else { 956 }
);

impl NOTIFYICONDATAW {
    /// 全零初始化，等价于 C 的 `NOTIFYICONDATAW nid{sizeof(nid)}`。
    pub fn zeroed() -> Self {
        // Safety: 全零对该结构体是合法状态（所有指针字段为 NULL）。
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

pub const MAX_PATH: usize = 260;
/// `INFINITE`
pub const INFINITE: DWORD = 0xFFFF_FFFF;
pub const WAIT_OBJECT_0: DWORD = 0;
pub const WAIT_TIMEOUT: DWORD = 0x102;

/// `PROCESS_QUERY_LIMITED_INFORMATION`
pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;

/// `CW_USEDEFAULT`
pub const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;

pub const SW_HIDE: i32 = 0;
pub const GW_HWNDNEXT: UINT = 2;
pub const GW_CHILD: UINT = 5;
pub const SW_SHOW: i32 = 5;
pub const SW_RESTORE: i32 = 9;
/// 使用 STARTUPINFO 里指定的显示方式（等价于 `wWinMain` 收到的 `nCmdShow`）。
pub const SW_SHOWDEFAULT: i32 = 10;
pub const SW_SHOWNORMAL: i32 = 1;

pub const SM_CXSMICON: i32 = 49;
pub const SM_CYSMICON: i32 = 50;

pub const COLOR_BTNFACE: i32 = 15;
/// `DEFAULT_GUI_FONT`
pub const DEFAULT_GUI_FONT: i32 = 17;

pub const IDC_ARROW: u16 = 32512;
pub const IDC_WAIT: u16 = 32513;

/// `IMAGE_ICON`
pub const IMAGE_ICON: UINT = 1;
pub const LR_DEFAULTSIZE: UINT = 0x0000_0040;
pub const LR_SHARED: UINT = 0x0000_8000;

// 窗口样式
pub const WS_OVERLAPPED: DWORD = 0x0000_0000;
pub const WS_CAPTION: DWORD = 0x00C0_0000;
pub const WS_SYSMENU: DWORD = 0x0008_0000;
pub const WS_MINIMIZEBOX: DWORD = 0x0002_0000;
pub const WS_CHILD: DWORD = 0x4000_0000;
pub const WS_VISIBLE: DWORD = 0x1000_0000;
pub const WS_VSCROLL: DWORD = 0x0020_0000;
pub const WS_EX_CLIENTEDGE: DWORD = 0x0000_0200;

// 编辑框样式
pub const ES_MULTILINE: DWORD = 0x0004;
pub const ES_AUTOVSCROLL: DWORD = 0x0040;
pub const ES_READONLY: DWORD = 0x0800;

// 静态文本样式
pub const SS_LEFT: DWORD = 0x0000_0000;

// 按钮样式
pub const BS_AUTOCHECKBOX: DWORD = 0x0000_0003;
pub const BST_CHECKED: WPARAM = 1;
pub const BST_UNCHECKED: WPARAM = 0;
/// `BM_GETCHECK`
pub const BM_GETCHECK: UINT = 0x00F0;
/// `BM_SETCHECK`
pub const BM_SETCHECK: UINT = 0x00F1;

// 下拉框样式与消息
pub const CBS_DROPDOWNLIST: DWORD = 0x0003;
/// `CB_ADDSTRING`
pub const CB_ADDSTRING: UINT = 0x0143;
/// `CB_GETCURSEL`
pub const CB_GETCURSEL: UINT = 0x0147;
/// `CB_RESETCONTENT`
pub const CB_RESETCONTENT: UINT = 0x014B;
/// `CB_SETCURSEL`
pub const CB_SETCURSEL: UINT = 0x014E;

// 窗口消息
pub const WM_CREATE: UINT = 0x0001;
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_SETFONT: UINT = 0x0030;
pub const WM_COMMAND: UINT = 0x0111;
pub const WM_SYSCOMMAND: UINT = 0x0112;
pub const WM_TIMER: UINT = 0x0113;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_LBUTTONDBLCLK: UINT = 0x0203;
pub const WM_RBUTTONUP: UINT = 0x0205;
/// `WM_APP`
pub const WM_APP: UINT = 0x8000;
/// `SC_MINIMIZE`（`WM_SYSCOMMAND` 的 wParam 掩码为 0xFFF0）
pub const SC_MINIMIZE: WPARAM = 0xF020;
pub const SC_MASK: WPARAM = 0xFFF0;

// 消息框
pub const MB_OK: UINT = 0x0000_0000;
pub const MB_ICONERROR: UINT = 0x0000_0010;
pub const MB_ICONINFORMATION: UINT = 0x0000_0040;

// 菜单
pub const MF_STRING: UINT = 0x0000_0000;
pub const MF_POPUP: UINT = 0x0000_0010;
pub const MF_SEPARATOR: UINT = 0x0000_0800;
pub const MF_CHECKED: UINT = 0x0000_0008;
pub const MF_BYPOSITION: UINT = 0x0000_0400;
pub const TPM_LEFTALIGN: UINT = 0x0000;
pub const TPM_BOTTOMALIGN: UINT = 0x0020;
/// 让 `TrackPopupMenu` 直接返回菜单项 ID 而不发送 `WM_COMMAND`。
pub const TPM_RETURNCMD: UINT = 0x0100;

/// `ICC_STANDARD_CLASSES`
pub const ICC_STANDARD_CLASSES: DWORD = 0x0000_4000;

// 托盘图标
pub const NIM_ADD: DWORD = 0x0000_0000;
pub const NIM_DELETE: DWORD = 0x0000_0002;
pub const NIF_MESSAGE: UINT = 0x0000_0001;
pub const NIF_ICON: UINT = 0x0000_0002;
pub const NIF_TIP: UINT = 0x0000_0004;

// ---------------------------------------------------------------------------
// 外部函数
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(lpModuleName: LPCWSTR) -> HMODULE;
    pub fn GetModuleFileNameW(hModule: HMODULE, lpFilename: LPWSTR, nSize: DWORD) -> DWORD;
    pub fn GetTickCount64() -> u64;
    pub fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
    pub fn GetLastError() -> DWORD;

    pub fn CreateEventW(
        lpEventAttributes: LPVOID,
        bManualReset: BOOL,
        bInitialState: BOOL,
        lpName: LPCWSTR,
    ) -> HANDLE;
    pub fn SetEvent(hEvent: HANDLE) -> BOOL;
    pub fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;

    pub fn DeleteFileW(lpFileName: LPCWSTR) -> BOOL;

    pub fn OpenProcess(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE;
    pub fn QueryFullProcessImageNameW(
        hProcess: HANDLE,
        dwFlags: DWORD,
        lpExeName: LPWSTR,
        lpdwSize: *mut DWORD,
    ) -> BOOL;

    pub fn GetPrivateProfileSectionW(lpAppName: LPCWSTR, lpReturnedString: LPWSTR, nSize: DWORD, lpFileName: LPCWSTR) -> DWORD;
    pub fn GetPrivateProfileStringW(lpAppName: LPCWSTR, lpKeyName: LPCWSTR, lpDefault: LPCWSTR, lpReturnedString: LPWSTR, nSize: DWORD, lpFileName: LPCWSTR) -> DWORD;
    pub fn WritePrivateProfileStringW(lpAppName: LPCWSTR, lpKeyName: LPCWSTR, lpString: LPCWSTR, lpFileName: LPCWSTR) -> BOOL;
    pub fn GetUserDefaultUILanguage() -> u16;
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassExW(param0: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        dwExStyle: DWORD,
        lpClassName: LPCWSTR,
        lpWindowName: LPCWSTR,
        dwStyle: DWORD,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: LPVOID,
    ) -> HWND;
    pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
    pub fn UpdateWindow(hWnd: HWND) -> BOOL;
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;

    pub fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: UINT, wMsgFilterMax: UINT)
        -> BOOL;
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn PostMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn SendMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;

    pub fn SetWindowTextW(hWnd: HWND, lpString: LPCWSTR) -> BOOL;
    pub fn GetWindowTextW(hWnd: HWND, lpString: LPWSTR, nMaxCount: i32) -> i32;
    pub fn GetWindow(hWnd: HWND, uCmd: UINT) -> HWND;
    pub fn EnableWindow(hWnd: HWND, bEnable: BOOL) -> BOOL;

    pub fn SetTimer(hWnd: HWND, nIDEvent: UINT_PTR, uElapse: UINT, lpTimerFunc: LPVOID)
        -> UINT_PTR;
    pub fn KillTimer(hWnd: HWND, uIDEvent: UINT_PTR) -> BOOL;

    pub fn MessageBoxW(hWnd: HWND, lpText: LPCWSTR, lpCaption: LPCWSTR, uType: UINT) -> i32;

    pub fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: LPCWSTR) -> HCURSOR;
    pub fn SetCursor(hCursor: HCURSOR) -> HCURSOR;
    pub fn LoadImageW(
        hInst: HINSTANCE,
        name: LPCWSTR,
        type_: UINT,
        cx: i32,
        cy: i32,
        fuLoad: UINT,
    ) -> HANDLE;
    pub fn GetSystemMetrics(nIndex: i32) -> i32;

    pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
    pub fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
    pub fn FindWindowW(lpClassName: LPCWSTR, lpWindowName: LPCWSTR) -> HWND;
    pub fn IsIconic(hWnd: HWND) -> BOOL;

    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(hMenu: HMENU, uFlags: UINT, uIDNewItem: UINT_PTR, lpNewItem: LPCWSTR)
        -> BOOL;
    pub fn TrackPopupMenu(
        hMenu: HMENU,
        uFlags: UINT,
        x: i32,
        y: i32,
        nReserved: i32,
        hWnd: HWND,
        prcRect: *const RECT,
    ) -> BOOL;
    pub fn DestroyMenu(hMenu: HMENU) -> BOOL;

    pub fn AdjustWindowRect(lpRect: *mut RECT, dwStyle: DWORD, bMenu: BOOL) -> BOOL;
    pub fn GetSystemMenu(hWnd: HWND, bRevert: BOOL) -> HMENU;
    pub fn RemoveMenu(hMenu: HMENU, uPosition: UINT, uFlags: UINT) -> BOOL;
    pub fn GetMenuItemCount(hMenu: HMENU) -> i32;
    pub fn EnumChildWindows(hWndParent: HWND, lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lParam: LPARAM) -> BOOL;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    /// 取系统库存对象（例如 `DEFAULT_GUI_FONT`）。注意它在 gdi32 而不是 user32。
    pub fn GetStockObject(i: i32) -> HANDLE;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub fn InitCommonControlsEx(picce: *const INITCOMMONCONTROLSEX) -> BOOL;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn Shell_NotifyIconW(dwMessage: DWORD, lpData: *mut NOTIFYICONDATAW) -> BOOL;
    pub fn ShellExecuteW(
        hwnd: HWND,
        lpOperation: LPCWSTR,
        lpFile: LPCWSTR,
        lpParameters: LPCWSTR,
        lpDirectory: LPCWSTR,
        nShowCmd: i32,
    ) -> HINSTANCE;
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub fn CoInitializeEx(pvReserved: LPVOID, dwCoInit: DWORD) -> HRESULT;
    pub fn CoUninitialize();
    pub fn CoCreateInstance(
        rclsid: *const GUID,
        pUnkOuter: *mut core::ffi::c_void,
        dwClsContext: DWORD,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> HRESULT;
    pub fn CoTaskMemFree(pv: LPVOID);
    pub fn PropVariantClear(pvar: *mut PROPVARIANT) -> HRESULT;
}

#[link(name = "avrt")]
unsafe extern "system" {
    pub fn AvSetMmThreadCharacteristicsW(TaskName: LPCWSTR, TaskIndex: *mut DWORD) -> HANDLE;
    pub fn AvRevertMmThreadCharacteristics(AvrtHandle: HANDLE) -> BOOL;
}

// ---------------------------------------------------------------------------
// RAII 小工具
// ---------------------------------------------------------------------------

/// `CoInitializeEx` / `CoUninitialize` 配对，离开作用域自动反初始化。
pub struct ComApartment {
    initialized: bool,
}

impl ComApartment {
    /// 以单元线程模型初始化（UI 线程用）。
    ///
    /// # Safety
    /// 必须在真正要使用 COM 的线程上调用。
    pub unsafe fn sta() -> Self {
        let hr = CoInitializeEx(core::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        Self { initialized: succeeded(hr) }
    }

    /// 以多线程模型初始化（采集/编码线程用）。
    ///
    /// # Safety
    /// 必须在真正要使用 COM 的线程上调用。
    pub unsafe fn mta() -> Self {
        let hr = CoInitializeEx(core::ptr::null_mut(), COINIT_MULTITHREADED);
        Self { initialized: succeeded(hr) }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // Safety: 与上面成功的 CoInitializeEx 配对。
            unsafe { CoUninitialize() };
        }
    }
}
