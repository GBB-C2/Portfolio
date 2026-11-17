//! Optional native resources, compiled with the target Windows SDK tools.
//! The drawing application itself keeps zero Cargo dependencies.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    for file in [
        "assets/portfolio.rc",
        "assets/portfolio.ico",
        "assets/portfolio.manifest",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    for variable in [
        "WINDRES",
        "WINDRES_PREPROCESSOR",
        "RC",
        "CC",
        "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let assets = manifest_dir.join("assets");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let (mut command, output_file, tool_label) = if target_env == "gnu" {
        let compiler =
            env::var_os("WINDRES").unwrap_or_else(|| "x86_64-w64-mingw32-windres".into());
        let mut command = Command::new(&compiler);
        let output_file = out.join("portfolio_resources.o");
        command
            .current_dir(&assets)
            .args([
                "--input",
                "portfolio.rc",
                "--input-format",
                "rc",
                "--output-format",
                "coff",
            ])
            .arg("--output")
            .arg(&output_file);
        // Relocatable MinGW toolchains may expose gcc-win32 instead of the
        // usual gcc alias. Accept an explicit preprocessor or reuse Cargo's gcc.
        let preprocessor = env::var_os("WINDRES_PREPROCESSOR")
            .or_else(|| {
                env::var_os("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER")
                    .filter(|value| value.to_string_lossy().contains("gcc"))
            })
            .or_else(|| env::var_os("CC"));
        if let Some(preprocessor) = preprocessor {
            command.arg("--preprocessor").arg(preprocessor).args([
                "--preprocessor-arg=-E",
                "--preprocessor-arg=-xc",
                "--preprocessor-arg=-DRC_INVOKED",
            ]);
        }
        (
            command,
            output_file,
            compiler.to_string_lossy().into_owned(),
        )
    } else if target_env == "msvc" {
        let compiler = env::var_os("RC").unwrap_or_else(|| "rc.exe".into());
        let mut command = Command::new(&compiler);
        let output_file = out.join("portfolio_resources.res");
        command
            .current_dir(&assets)
            .args(["/nologo", "/fo"])
            .arg(&output_file)
            .arg("portfolio.rc");
        (
            command,
            output_file,
            compiler.to_string_lossy().into_owned(),
        )
    } else {
        println!("cargo:warning=Portfolio native resources are unavailable for this Windows toolchain; the editor can still run.");
        return;
    };

    match command.output() {
        Ok(result) if result.status.success() && output_file.is_file() => {
            println!(
                "cargo:rustc-link-arg-bin=Portfolio={}",
                output_file.display()
            );
        }
        Ok(result) => {
            let details = String::from_utf8_lossy(&result.stderr).replace(['\n', '\r'], " ");
            println!("cargo:warning=Could not embed Portfolio's icon and manifest with {tool_label}: {details}. Native resources are optional; DPI initialization and a system icon remain available.");
        }
        Err(error) => {
            println!("cargo:warning=Optional resource compiler {tool_label} is unavailable ({error}). Install the Windows SDK/MinGW resource tools, or set RC/WINDRES, to embed the application icon and manifest.");
        }
    }
}
