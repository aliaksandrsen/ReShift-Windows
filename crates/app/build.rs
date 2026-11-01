//! Compiles the Win32 resources (manifest, icons, version info) with rc.exe from the Windows SDK
//! and hands the .res file to the linker. No build dependencies: the SDK is already required
//! by the MSVC toolchain.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type BuildResult<T> = Result<T, Box<dyn Error>>;

fn main() -> BuildResult<()> {
    println!("cargo:rerun-if-changed=res");
    println!("cargo:rerun-if-env-changed=RESHIFT_RC");

    if env::var("CARGO_CFG_TARGET_OS")? != "windows" || env::var("CARGO_CFG_TARGET_ENV")? != "msvc"
    {
        return Ok(());
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let script = render_resource_script(&manifest_dir.join("res"))?;
    let rc_path = out_dir.join("app.rc");
    let res_path = out_dir.join("app.res");
    fs::write(&rc_path, script)?;

    let rc_exe = find_rc_exe()?;
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", rc_exe.display()))?;
    if !status.success() {
        return Err(format!("{} failed with {status}", rc_exe.display()).into());
    }

    // link.exe takes compiled resources as a regular input file
    println!("cargo:rustc-link-arg-bins={}", res_path.display());
    Ok(())
}

/// Fills the placeholders of res/app.rc: the absolute resource folder and the crate version.
fn render_resource_script(res_dir: &Path) -> BuildResult<String> {
    let template = fs::read_to_string(res_dir.join("app.rc"))?;
    let escaped_dir = res_dir.display().to_string().replace('\\', "\\\\");
    let version_comma = format!(
        "{},{},{}",
        env::var("CARGO_PKG_VERSION_MAJOR")?,
        env::var("CARGO_PKG_VERSION_MINOR")?,
        env::var("CARGO_PKG_VERSION_PATCH")?
    );
    Ok(template
        .replace("@RES_DIR@", &escaped_dir)
        .replace("@VERSION_COMMA@", &version_comma)
        .replace("@VERSION@", &env::var("CARGO_PKG_VERSION")?))
}

/// rc.exe of the newest installed Windows 10/11 SDK (or RESHIFT_RC if set).
fn find_rc_exe() -> BuildResult<PathBuf> {
    if let Ok(explicit) = env::var("RESHIFT_RC") {
        return Ok(PathBuf::from(explicit));
    }

    let host_arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };
    let bin = kits_root().join("bin");
    let mut candidates: Vec<(Vec<u32>, PathBuf)> = fs::read_dir(&bin)
        .map_err(|e| format!("Windows SDK not found in {}: {e}", bin.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version: Vec<u32> = name
                .split('.')
                .map(|part| part.parse().ok())
                .collect::<Option<_>>()?;
            let rc = entry.path().join(host_arch).join("rc.exe");
            rc.is_file().then_some((version, rc))
        })
        .collect();
    candidates.sort();
    candidates.pop().map(|(_, rc)| rc).ok_or_else(|| {
        format!(
            "rc.exe not found under {} - install the Windows SDK",
            bin.display()
        )
        .into()
    })
}

/// KitsRoot10 from the registry, or the default location.
fn kits_root() -> PathBuf {
    let query = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
            "/v",
            "KitsRoot10",
        ])
        .output();
    if let Ok(output) = query {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = text
            .lines()
            .find_map(|line| {
                line.split_once("REG_SZ")
                    .map(|(_, value)| value.trim().to_owned())
            })
            .filter(|path| !path.is_empty())
        {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10")
}
