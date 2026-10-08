//! Win32 / COM / Media Foundation 的原始 FFI 声明。
//!
//! 这里刻意不使用 `windows` crate：目标环境访问不到 crates.io，所以所有接口的
//! COM vtable 都按 Windows SDK 头文件（10.0.26100.0）的声明顺序手写，并直接
//! 链接系统 DLL（user32 / ole32 / mfplat / mfreadwrite / avrt / comctl32 / shell32）。
//!
//! # vtable 顺序为什么必须逐字对齐
//!
//! COM 接口的方法只通过 vtable 偏移调用，方法名对 ABI 毫无意义。因此每个
//! `*Vtbl` 结构中函数指针的**顺序**必须与 SDK 头文件里 `MIDL_INTERFACE` 之后
//! 的声明顺序完全一致（前 3 项固定是 `QueryInterface` / `AddRef` / `Release`）。
//! 每个接口上方都标注了对应的头文件，便于核对。

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
// 与 Windows SDK 的类型名保持一致（LPCGUID / LPCWSTR ...），不改成 Rust 风格
#![allow(clippy::upper_case_acronyms)]

use core::ffi::c_void;
use core::ptr;

pub mod mf;
pub mod system;
pub mod wasapi;

// ---------------------------------------------------------------------------
// 基础类型别名（与 Windows SDK 的 C 定义一一对应）
// ---------------------------------------------------------------------------

/// `HRESULT`：成功为 `>= 0`，失败为 `< 0`。
pub type HRESULT = i32;
/// `BOOL`（4 字节，非 `bool`）。
pub type BOOL = i32;
pub type DWORD = u32;
pub type UINT = u32;
pub type UINT32 = u32;
pub type UINT64 = u64;
pub type WORD = u16;
pub type LONGLONG = i64;
/// `REFERENCE_TIME`（100ns 单位）。
pub type REFERENCE_TIME = i64;
pub type LPVOID = *mut c_void;
pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HMENU = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HFONT = *mut c_void;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type LPCWSTR = *const u16;
pub type LPWSTR = *mut u16;
pub type UINT_PTR = usize;

pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;
pub const S_OK: HRESULT = 0;
pub const S_FALSE: HRESULT = 1;
/// `E_FAIL`
pub const E_FAIL: HRESULT = 0x8000_4005u32 as HRESULT;
/// `E_NOINTERFACE`
pub const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as HRESULT;

#[inline]
pub const fn succeeded(hr: HRESULT) -> bool {
    hr >= 0
}

#[inline]
pub const fn failed(hr: HRESULT) -> bool {
    hr < 0
}

/// 把整数资源 ID 当成字符串指针使用，等价于 C 的 `MAKEINTRESOURCEW(id)`。
#[inline]
pub const fn make_int_resource(id: u16) -> LPCWSTR {
    id as usize as LPCWSTR
}

/// 生成 `"<做什么> 失败 (HRESULT=0x????????)"` 形式的错误信息，对应 C++ 版
/// 各文件里同名的 `HrMessage`。
pub fn hr_message(what: &str, hr: HRESULT) -> String {
    format!("{what} (HRESULT=0x{:08X})", hr as u32)
}

// ---------------------------------------------------------------------------
// COM 基础：GUID / PROPERTYKEY / PROPVARIANT
// ---------------------------------------------------------------------------

/// `GUID`（= `IID` = `CLSID`）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self { data1, data2, data3, data4 }
    }
}

/// `PROPERTYKEY`（fmtid + pid）。
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PROPERTYKEY {
    pub fmtid: GUID,
    pub pid: u32,
}

/// `PROPVARIANT` 的联合体部分。
///
/// x64 上联合体宽 16 字节（指针 8 字节，或 `cElems + 指针` 这类 4+pad+8 的组合），
/// x86 上宽 8 字节；用 `[usize; 2]` / `[usize; 1]` 表达可以自动跟随指针宽度。
#[repr(C)]
#[derive(Copy, Clone)]
pub union PROPVARIANT_UNION {
    pub ll_val: i64,
    pub pwsz_val: LPWSTR,
    pub punk_val: *mut c_void,
    pub pad: [usize; 2],
}

/// `PROPVARIANT`：`vt` + 3 个保留字 + 16 字节联合体 = x64 上 24 字节。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PROPVARIANT {
    pub vt: u16,
    pub w_reserved1: u16,
    pub w_reserved2: u16,
    pub w_reserved3: u16,
    pub data: PROPVARIANT_UNION,
}

/// `VT_EMPTY` / `VT_LPWSTR`（`VARENUM` 成员）。
pub const VT_EMPTY: u16 = 0;
pub const VT_LPWSTR: u16 = 31;

impl PROPVARIANT {
    /// 等价于 C 的 `PropVariantInit()`（清零）。
    pub const fn zeroed() -> Self {
        Self { vt: VT_EMPTY, w_reserved1: 0, w_reserved2: 0, w_reserved3: 0, data: PROPVARIANT_UNION { pad: [0; 2] } }
    }
}

// 布局自检：与 C 的 sizeof(PROPVARIANT) 一致，避免联合体定义写错。
const _: () = assert!(core::mem::size_of::<PROPVARIANT>() == if cfg!(target_pointer_width = "64") { 24 } else { 16 });
const _: () = assert!(core::mem::size_of::<GUID>() == 16);
const _: () = assert!(core::mem::size_of::<PROPERTYKEY>() == 20);

/// `CLSCTX_ALL`
pub const CLSCTX_ALL: DWORD = 0x17;
/// `COINIT_APARTMENTTHREADED`
pub const COINIT_APARTMENTTHREADED: DWORD = 0x2;
/// `COINIT_MULTITHREADED`
pub const COINIT_MULTITHREADED: DWORD = 0x0;

// ---------------------------------------------------------------------------
// COM 智能指针
// ---------------------------------------------------------------------------

/// 所有手写 COM 接口共有的能力。
///
/// # 为什么方法都取 `&self` 而不是 `self`
///
/// `IFoo` 是 `#[repr(C)]` 且**只含一个 vtable 指针字段**的结构体，因此
/// "`IFoo` 所在内存的地址"就是 COM 接口指针本身（即 `this`）。若方法按值接收
/// `self`，`self` 会变成栈上的一份拷贝，把拷贝的地址当 `this` 传进 COM 会让
/// `Release`、`CONTAINING_RECORD` 之类依赖对象地址的实现写坏内存。
/// 所以所有方法都通过 `&self` 取地址。
///
/// 安全性：实现者必须保证 `Self` 是 `#[repr(C)]` 且只含一个 vtable 指针字段。
pub trait Com {
    const IID: GUID;

    /// 取原始接口指针（不转移所有权）。
    fn as_raw(&self) -> *mut c_void;
    /// # Safety
    /// 直接调用 vtable，调用方需保证接口仍然有效。
    unsafe fn add_ref(&self) -> u32;
    /// # Safety
    /// 直接调用 vtable，调用方需保证接口仍然有效。
    unsafe fn release(&self) -> u32;
    /// # Safety
    /// `out` 必须是可写的接口指针出参。
    unsafe fn query_interface_raw(&self, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
}

/// COM 接口指针的 RAII 包装：离开作用域时自动 `Release`。
///
/// 不做 `AddRef`/`Release` 之外的任何线程处理：COM 对象的线程规则由调用方遵守
/// （本项目里每个 COM 对象都只在自己创建它的线程上使用）。
pub struct ComPtr<T: Com> {
    ptr: *mut T,
}

impl<T: Com> ComPtr<T> {
    /// 空指针。
    pub fn null() -> Self {
        Self { ptr: ptr::null_mut() }
    }

    /// 从原始接口指针接管所有权（不再 AddRef）。
    ///
    /// # Safety
    /// `ptr` 为合法接口指针，且调用方移交一个引用；可为空。
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// 从 `void*` 形式的接口指针接管所有权。
    ///
    /// # Safety
    /// 同 [`ComPtr::from_raw`]。
    pub unsafe fn from_void(ptr: *mut c_void) -> Self {
        Self { ptr: ptr as *mut T }
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }

    /// 交给 C 函数的出参：`T**`。
    pub fn as_out_ptr(&mut self) -> *mut *mut T {
        &mut self.ptr
    }

    /// 交给 C 函数的出参：`void**`（`Activate` / `CoCreateInstance` 等）。
    pub fn as_void_out_ptr(&mut self) -> *mut *mut c_void {
        &mut self.ptr as *mut *mut T as *mut *mut c_void
    }

    /// 借用接口；空指针时返回 `None`。
    pub fn as_ref(&self) -> Option<&T> {
        if self.ptr.is_null() {
            None
        } else {
            // Safety: 非空即由构造方保证是合法接口指针，且 ComPtr 持有其生命周期。
            Some(unsafe { &*self.ptr })
        }
    }

    /// 借用接口；空指针时 panic（用于"此处不可能为空"的场景）。
    ///
    /// # Panics
    /// 接口指针为空时 panic。
    pub fn expect_ref(&self) -> &T {
        self.as_ref().expect("COM 接口指针为 NULL")
    }

    /// `QueryInterface`，返回另一个接口的 RAII 指针。
    ///
    /// # Errors
    /// 接口不支持时返回 `E_NOINTERFACE`。
    pub fn query<U: Com>(&self) -> Result<ComPtr<U>, HRESULT> {
        if self.ptr.is_null() {
            return Err(E_NOINTERFACE);
        }
        let mut out: *mut c_void = ptr::null_mut();
        // Safety: self.ptr 非空且有效；out 是合法的出参。
        let hr = unsafe { (*self.ptr).query_interface_raw(&U::IID, &mut out) };
        if failed(hr) {
            Err(hr)
        } else {
            // Safety: 成功时 out 是新接口指针，带一个引用。
            Ok(unsafe { ComPtr::from_void(out) })
        }
    }

    /// 放弃所有权，返回原始指针（不会 Release，交给调用方负责）。
    pub fn into_raw(self) -> *mut T {
        let p = self.ptr;
        core::mem::forget(self);
        p
    }
}

impl<T: Com> Default for ComPtr<T> {
    fn default() -> Self {
        Self::null()
    }
}

impl<T: Com> Drop for ComPtr<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // Safety: ComPtr 独占一个引用，这里正好释放它。
            unsafe {
                (*self.ptr).release();
            }
            self.ptr = ptr::null_mut();
        }
    }
}

impl<T: Com> core::fmt::Debug for ComPtr<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ComPtr({:p})", self.ptr)
    }
}

// ---------------------------------------------------------------------------
// 可跨线程传递的窗口句柄
// ---------------------------------------------------------------------------

/// 可以跨线程传递的 `HWND`。
///
/// `PostMessage` / `SendMessage` 本身是线程安全的，窗口句柄只是一个标识，
/// 因此采集线程可以持有它并向 UI 线程投递错误通知。
#[derive(Copy, Clone)]
pub struct SendableHwnd(pub HWND);

// Safety: HWND 是内核对象标识，PostMessage 允许跨线程使用。
unsafe impl Send for SendableHwnd {}
unsafe impl Sync for SendableHwnd {}

// ---------------------------------------------------------------------------
// 接口声明宏
// ---------------------------------------------------------------------------

/// 声明一个 COM 接口（从 `IUnknown` 直接派生）。
///
/// 用法：
/// ```ignore
/// com_interface! {
///     /// 文档注释
///     IFoo, IFooVtbl, GUID::new(0x...);
///     fn bar(self_param: *mut c_void) -> HRESULT;
///     fn baz() -> HRESULT;
/// }
/// ```
/// 生成 `IFoo`（含 vtable 指针）与 `IFooVtbl`（3 个 IUnknown 槽位 + 各方法），
/// 以及 `IFoo::IID`、`as_raw()`、`add_ref()`、`release()` 和各方法的安全包装。
#[macro_export]
macro_rules! com_interface {
    (
        $(#[$meta:meta])*
        $name:ident, $vtbl:ident, $iid:expr;
        $(
            $(#[$m:meta])*
            fn $method:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty;
        )*
    ) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Copy, Clone)]
        pub struct $name {
            /// 指向 COM vtable 的指针，同时也正是 COM 接口指针本身。
            pub vtbl: *const $vtbl,
        }

        /// COM vtable。前 3 项固定为 `IUnknown`，其后顺序与 SDK 头文件一致。
        #[repr(C)]
        pub struct $vtbl {
            pub query_interface: unsafe extern "system" fn(
                *mut ::core::ffi::c_void,
                *const $crate::ffi::GUID,
                *mut *mut ::core::ffi::c_void,
            ) -> $crate::ffi::HRESULT,
            pub add_ref: unsafe extern "system" fn(*mut ::core::ffi::c_void) -> u32,
            pub release: unsafe extern "system" fn(*mut ::core::ffi::c_void) -> u32,
            $(
                $(#[$m])*
                pub $method: unsafe extern "system" fn(
                    *mut ::core::ffi::c_void,
                    $($ty),*
                ) -> $ret,
            )*
        }

        impl $name {
            /// 以原始接口指针形式返回（不转移所有权，不改变引用计数）。
            #[inline]
            pub fn as_raw(&self) -> *mut ::core::ffi::c_void {
                self as *const Self as *mut ::core::ffi::c_void
            }

            /// `AddRef`。
            ///
            /// # Safety
            /// 接口必须仍然有效。
            #[inline]
            pub unsafe fn add_ref(&self) -> u32 {
                ((*self.vtbl).add_ref)(self.as_raw())
            }

            /// `Release`。正常路径请依靠 [`ComPtr`] 自动管理。
            ///
            /// # Safety
            /// 调用方必须确实持有该接口的一个引用。
            #[inline]
            pub unsafe fn release(&self) -> u32 {
                ((*self.vtbl).release)(self.as_raw())
            }

            $(
                $(#[$m])*
                ///
                /// # Safety
                /// 参数必须是该 COM 方法要求的合法值，且接口有效。
                #[inline]
                pub unsafe fn $method(&self $(, $arg: $ty)*) -> $ret {
                    ((*self.vtbl).$method)(self.as_raw() $(, $arg)*)
                }
            )*
        }

        impl $crate::ffi::Com for $name {
            const IID: $crate::ffi::GUID = $iid;

            #[inline]
            fn as_raw(&self) -> *mut ::core::ffi::c_void {
                self as *const Self as *mut ::core::ffi::c_void
            }

            #[inline]
            unsafe fn query_interface_raw(
                &self,
                iid: *const $crate::ffi::GUID,
                out: *mut *mut ::core::ffi::c_void,
            ) -> $crate::ffi::HRESULT {
                ((*self.vtbl).query_interface)(self.as_raw(), iid, out)
            }

            #[inline]
            unsafe fn add_ref(&self) -> u32 {
                ((*self.vtbl).add_ref)(self.as_raw())
            }

            #[inline]
            unsafe fn release(&self) -> u32 {
                ((*self.vtbl).release)(self.as_raw())
            }
        }
    };
}

/// 声明一个**继承**自其它 COM 接口的接口。
///
/// vtable 布局 = 基接口 vtable（位于偏移 0）+ 本接口新增方法。这样不必把基接口的
/// 方法（例如 `IMFAttributes` 的 30 个）在每个派生接口里重复写一遍。
///
/// `inherited { ... }` 列出需要在派生类型上直接可用的基接口方法（会生成转发包装）。
#[macro_export]
macro_rules! com_interface_ext {
    (
        $(#[$meta:meta])*
        $name:ident, $vtbl:ident, $iid:expr, $base_vtbl:ty;
        inherited { $($(#[$im_meta:meta])* fn $im:ident($($ia:ident : $it:ty),* $(,)?) -> $iret:ty;)* }
        own { $($(#[$om_meta:meta])* fn $om:ident($($oa:ident : $ot:ty),* $(,)?) -> $oret:ty;)* }
    ) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Copy, Clone)]
        pub struct $name {
            /// 指向 COM vtable 的指针，同时也正是 COM 接口指针本身。
            pub vtbl: *const $vtbl,
        }

        /// COM vtable：基接口部分 + 本接口新增方法。
        #[repr(C)]
        pub struct $vtbl {
            /// 基接口的 vtable（必须位于偏移 0）。
            pub base: $base_vtbl,
            $(
                $(#[$om_meta])*
                pub $om: unsafe extern "system" fn(
                    *mut ::core::ffi::c_void,
                    $($ot),*
                ) -> $oret,
            )*
        }

        impl $name {
            /// 以原始接口指针形式返回（不转移所有权，不改变引用计数）。
            #[inline]
            pub fn as_raw(&self) -> *mut ::core::ffi::c_void {
                self as *const Self as *mut ::core::ffi::c_void
            }

            /// `AddRef`。
            ///
            /// # Safety
            /// 接口必须仍然有效。
            #[inline]
            pub unsafe fn add_ref(&self) -> u32 {
                ((*self.vtbl).base.add_ref)(self.as_raw())
            }

            /// `Release`。正常路径请依靠 [`ComPtr`] 自动管理。
            ///
            /// # Safety
            /// 调用方必须确实持有该接口的一个引用。
            #[inline]
            pub unsafe fn release(&self) -> u32 {
                ((*self.vtbl).base.release)(self.as_raw())
            }

            $(
                $(#[$im_meta])*
                ///
                /// # Safety
                /// 参数必须是该 COM 方法要求的合法值，且接口有效。
                #[inline]
                pub unsafe fn $im(&self $(, $ia: $it)*) -> $iret {
                    ((*self.vtbl).base.$im)(self.as_raw() $(, $ia)*)
                }
            )*

            $(
                $(#[$om_meta])*
                ///
                /// # Safety
                /// 参数必须是该 COM 方法要求的合法值，且接口有效。
                #[inline]
                pub unsafe fn $om(&self $(, $oa: $ot)*) -> $oret {
                    ((*self.vtbl).$om)(self.as_raw() $(, $oa)*)
                }
            )*
        }

        impl $crate::ffi::Com for $name {
            const IID: $crate::ffi::GUID = $iid;

            #[inline]
            fn as_raw(&self) -> *mut ::core::ffi::c_void {
                self as *const Self as *mut ::core::ffi::c_void
            }

            #[inline]
            unsafe fn query_interface_raw(
                &self,
                iid: *const $crate::ffi::GUID,
                out: *mut *mut ::core::ffi::c_void,
            ) -> $crate::ffi::HRESULT {
                ((*self.vtbl).base.query_interface)(self.as_raw(), iid, out)
            }

            #[inline]
            unsafe fn add_ref(&self) -> u32 {
                ((*self.vtbl).base.add_ref)(self.as_raw())
            }

            #[inline]
            unsafe fn release(&self) -> u32 {
                ((*self.vtbl).base.release)(self.as_raw())
            }
        }
    };
}
