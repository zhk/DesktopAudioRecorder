//! 构建脚本：嵌入「应用图标」资源 + 「通用控件 v6」清单。
//!
//! 对应 C++ 版里的两件事：
//!   1. `src/app.rc` 中的 `IDI_APP ICON "icon.ico"`（图标资源）；
//!   2. `#pragma comment(linker, "/manifestdependency:...Common-Controls...")`
//!      （让通用控件使用系统主题）。
//!
//! Rust 没有 .rc 编译器，这里在构建期调用 Windows SDK 的 rc.exe 编译 app.rc，
//! 并把 .res 交给链接器；清单则通过 /MANIFESTDEPENDENCY 链接参数嵌入。
//! 找不到 rc.exe 时只警告不报错：程序仍可编译，只是没有自定义图标。

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=resource.h");
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if target_env == "msvc" {
        // 通用控件 v6：让按钮/下拉框/编辑框使用系统主题。
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );

        match compile_resource_script(&target_arch) {
            Ok(Some(res)) => println!("cargo:rustc-link-arg={}", res.display()),
            Ok(None) => println!(
                "cargo:warning=未找到 rc.exe，跳过图标资源（exe 将使用系统默认图标）。\
                 安装 Windows SDK 后重新构建即可。"
            ),
            Err(e) => println!("cargo:warning=编译 app.rc 失败：{e}（继续构建，仅缺少图标）"),
        }
    } else if target_env == "gnu" {
        // MinGW 工具链：用 windres。
        if let Some(()) = try_windres() {}
    }
}

/// 用 rc.exe 编译 app.rc，返回生成的 .res 路径。
fn compile_resource_script(target_arch: &str) -> Result<Option<PathBuf>, String> {
    let Some(rc) = find_rc(target_arch) else {
        return Ok(None);
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let res = out_dir.join("app.res");

    let output = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        // rc.exe 需要能找到 app.rc 里引用的 icon.ico，因此在清单目录下执行。
        .arg("app.rc")
        .current_dir(&manifest_dir)
        .output()
        .map_err(|e| format!("启动 {} 失败：{e}", rc.display()))?;

    if !output.status.success() {
        return Err(format!(
            "rc.exe 退出码 {:?}：{}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(Some(res))
}

/// 定位 rc.exe：先看 RC 环境变量，再到 PATH，最后到 Windows Kits 安装目录。
fn find_rc(target_arch: &str) -> Option<PathBuf> {
    if let Ok(rc) = env::var("RC") {
        let p = PathBuf::from(&rc);
        if p.is_file() {
            return Some(p);
        }
    }
    // PATH
    let exe = "rc.exe";
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Windows Kits\10\bin\<version>\<arch>\rc.exe（取最高版本）
    let sdk_arch = match target_arch {
        "x86_64" => "x64",
        "x86" => "x86",
        "aarch64" => "arm64",
        _ => "x64",
    };
    let kits_root = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let entries = std::fs::read_dir(kits_root).ok()?;
    let mut versions: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    for v in versions.into_iter().rev() {
        let cand = v.join(sdk_arch).join(exe);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// MinGW 下用 windres 编译资源（尽力而为，失败不影响构建）。
fn try_windres() -> Option<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?).canonicalize().ok()?;
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    let res = out_dir.join("app_res.o");
    let status = Command::new("windres")
        .arg("-I")
        .arg(&manifest_dir)
        .arg(format!("--output={}", res.display()))
        .arg(manifest_dir.join("app.rc"))
        .status()
        .ok()?;
    if status.success() {
        println!("cargo:rustc-link-arg={}", res.display());
        Some(())
    } else {
        None
    }
}
