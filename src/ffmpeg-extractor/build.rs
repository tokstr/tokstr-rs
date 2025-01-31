use std::env;
use std::path::{Path, PathBuf};
extern crate pkg_config;

fn main() {
    let target = env::var("TARGET").expect("No TARGET env var");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("No OUT_DIR env var"));

    let ffmpeg_libs_raw = env::var("FFMPEG_LIBS_PATH")
        .unwrap_or_else(|_| "3rd-party/ffmpeg-libs".to_string());
    let ffmpeg_libs_candidate = PathBuf::from(&ffmpeg_libs_raw);

    if !ffmpeg_libs_candidate.exists() {
        panic!("FFMPEG_LIBS_PATH does not exist: {}", ffmpeg_libs_candidate.display());
    }

    // If you truly need canonicalize afterward, do it here:
    let ffmpeg_libs_path = ffmpeg_libs_candidate
        .canonicalize()
        .expect("Could not canonicalize FFMPEG_LIBS_PATH");

    println!("cargo:warning=Using FFMPEG_LIBS_PATH = {}", ffmpeg_libs_path.display());


    // 1) Android
    if target.contains("android") {
        build_for_android(&ffmpeg_libs_path, &target, &out_dir);
    }
    // 2) iOS
    else if target.contains("apple-ios") {
        build_for_ios(&ffmpeg_libs_path, &target, &out_dir);
    }
    // 3) macOS
    else if target.contains("apple-darwin") {
        // We'll try static subdirectory first; if missing, fallback to pkg-config
        build_for_macos_or_fallback_pkgconfig(&ffmpeg_libs_path, &target, &out_dir);
    }
    // 4) Windows
    else if target.contains("windows") {
        build_for_windows(&ffmpeg_libs_path, &target, &out_dir);
    }
    // 5) Linux
    else if target.contains("linux") {
        // We'll try static subdirectory first; if missing, fallback to pkg-config
        build_for_linux_or_fallback_pkgconfig(&ffmpeg_libs_path, &target, &out_dir);
    }
    // Fallback
    else {
        panic!("Unsupported target: {}", target);
    }
}

/* ------------------------------------------------------------------------
   Android
   ------------------------------------------------------------------------ */

fn build_for_android(ffmpeg_libs_path: &Path, target: &str, out_dir: &Path) {
    // e.g. "arm64-v8a", "armeabi-v7a", ...
    let arch_subdir = if target.contains("aarch64") {
        "arm64-v8a"
    } else if target.contains("armv7") {
        "armeabi-v7a"
    } else if target.contains("x86_64") {
        "x86_64"
    } else if target.contains("i686") {
        "x86"
    } else {
        panic!("Unsupported Android target: {}", target);
    };

    let include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
    let lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");
    check_ffmpeg_paths(&include_dir, &lib_dir);
    link_ffmpeg_static(&lib_dir);

    // On Android, also link the log library
    println!("cargo:rustc-link-lib=log");

    // Now compile our C code and run bindgen:
    compile_c(&[&include_dir]);
    generate_bindings(&[&include_dir], out_dir);
}

/* ------------------------------------------------------------------------
   iOS
   ------------------------------------------------------------------------ */

fn build_for_ios(ffmpeg_libs_path: &Path, target: &str, out_dir: &Path) {
    // e.g. "ios_arm64", "ios_x86_64_sim", "ios_arm64_sim", ...
    let arch_subdir = if target == "aarch64-apple-ios" {
        "ios_arm64" // iOS device
    } else if target == "x86_64-apple-ios" {
        "ios_x86_64_sim" // iOS simulator (Intel)
    } else if target == "aarch64-apple-ios-sim" {
        "ios_arm64_sim" // iOS simulator (Apple Silicon)
    } else {
        panic!("Unsupported iOS target: {}", target);
    };

    let include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
    let lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");
    check_ffmpeg_paths(&include_dir, &lib_dir);
    link_ffmpeg_static(&lib_dir);

    // If needed, link iOS frameworks here (AVFoundation, etc.)
    // println!("cargo:rustc-link-lib=framework=AVFoundation");
    // ...

    compile_c(&[&include_dir]);
    generate_bindings(&[&include_dir], out_dir);
}

/* ------------------------------------------------------------------------
   macOS with fallback to pkg-config
   ------------------------------------------------------------------------ */

fn build_for_macos_or_fallback_pkgconfig(ffmpeg_libs_path: &Path, target: &str, out_dir: &Path) {
    // Attempt to use "macos_arm64" or "macos_x86_64" subdir if it exists:
    let arch_subdir = if target.contains("x86_64") {
        "macos_x86_64"
    } else if target.contains("aarch64") {
        "macos_arm64"
    } else {
        panic!("Unsupported macOS target: {}", target);
    };

    let include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
    let lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");

    if include_dir.is_dir() && lib_dir.is_dir() {
        // We found local static libs
        check_ffmpeg_paths(&include_dir, &lib_dir);
        link_ffmpeg_static(&lib_dir);

        // If needed, link frameworks here
        // println!("cargo:rustc-link-lib=framework=AVFoundation");
        // ...

        compile_c(&[&include_dir]);
        generate_bindings(&[&include_dir], out_dir);
    } else {
        // Attempt pkg-config fallback
        println!("cargo:warning=No local static FFmpeg found for macOS; trying pkg-config...");
        if !try_pkg_config_ffmpeg() {
            panic!("FFmpeg not found in subfolder or via pkg-config for macOS!");
        }
    }
}

/* ------------------------------------------------------------------------
   Windows
   ------------------------------------------------------------------------ */

fn build_for_windows(ffmpeg_libs_path: &Path, target: &str, out_dir: &Path) {
    // e.g. "win_x86_64", "win_i686", "win_arm64", ...
    let arch_subdir = if target.contains("x86_64") {
        "win_x86_64"
    } else if target.contains("i686") {
        "win_i686"
    } else if target.contains("aarch64") {
        "win_arm64"
    } else {
        panic!("Unsupported Windows target: {}", target);
    };

    let include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
    let lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");
    check_ffmpeg_paths(&include_dir, &lib_dir);

    // Link the static .lib files
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=avcodec");
    println!("cargo:rustc-link-lib=static=avdevice");
    println!("cargo:rustc-link-lib=static=avfilter");
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avutil");
    println!("cargo:rustc-link-lib=static=postproc");
    println!("cargo:rustc-link-lib=static=swresample");
    println!("cargo:rustc-link-lib=static=swscale");
    // Possibly link Win libs like user32, bcrypt, etc., if needed.

    compile_c(&[&include_dir]);
    generate_bindings(&[&include_dir], out_dir);
}

/* ------------------------------------------------------------------------
   Linux with fallback to pkg-config
   ------------------------------------------------------------------------ */

fn build_for_linux_or_fallback_pkgconfig(ffmpeg_libs_path: &Path, target: &str, out_dir: &Path) {
    let arch_subdir = if target.contains("x86_64") {
        "linux_x86_64"
    } else if target.contains("aarch64") {
        "linux_arm64"
    } else {
        panic!("Unsupported Linux target: {}", target);
    };

    let include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
    let lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");

    if include_dir.is_dir() && lib_dir.is_dir() {
        // We found local static libs
        check_ffmpeg_paths(&include_dir, &lib_dir);
        link_ffmpeg_static(&lib_dir);

        compile_c(&[&include_dir]);
        generate_bindings(&[&include_dir], out_dir);
    } else {
        // Attempt pkg-config fallback
        println!("cargo:warning=No local static FFmpeg found for Linux; trying pkg-config...");
        if !try_pkg_config_ffmpeg() {
            panic!("FFmpeg not found in subfolder or via pkg-config for Linux!");
        }
    }
}

/* ------------------------------------------------------------------------
   Helpers
   ------------------------------------------------------------------------ */

/// Panics if include_dir or lib_dir does not exist.
fn check_ffmpeg_paths(include_dir: &Path, lib_dir: &Path) {
    if !include_dir.is_dir() {
        panic!("FFmpeg include dir not found: {}", include_dir.display());
    }
    if !lib_dir.is_dir() {
        panic!("FFmpeg lib dir not found: {}", lib_dir.display());
    }
}

/// Prints cargo directives to link FFmpeg static libraries
/// with the names you'd expect for .a files (avcodec, avutil, etc.).
fn link_ffmpeg_static(lib_dir: &Path) {
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=avcodec");
    println!("cargo:rustc-link-lib=static=avdevice");
    println!("cargo:rustc-link-lib=static=avfilter");
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avutil");
    println!("cargo:rustc-link-lib=static=postproc");
    println!("cargo:rustc-link-lib=static=swresample");
    println!("cargo:rustc-link-lib=static=swscale");
}

/// Compile our C code, including all provided include paths.
fn compile_c(include_dirs: &[&Path]) {
    let mut cc_builder = cc::Build::new();
    cc_builder.file("c_src/extract_jpeg_frame.c");
    cc_builder.include("c_src");

    for inc in include_dirs {
        cc_builder.include(inc);
    }

    cc_builder.compile("extractframe");
}

/// Run bindgen with all provided include paths, writing to OUT_DIR/bindings.rs.
fn generate_bindings(include_dirs: &[&Path], out_dir: &Path) {
    let mut bindgen_builder = bindgen::Builder::default()
        .header("c_src/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    for inc in include_dirs {
        bindgen_builder = bindgen_builder.clang_arg(format!("-I{}", inc.display()));
    }

    let bindings = bindgen_builder
        .generate()
        .expect("Failed to generate FFmpeg bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings");
}

/// Attempt to link and find FFmpeg via pkg-config on macOS/Linux.
/// Returns `true` if successful, `false` otherwise.
///
/// You can adapt the list of libraries to match your needs (e.g., add or remove).
fn try_pkg_config_ffmpeg() -> bool {
    let pkgs = [
        "libavcodec",
        "libavdevice",
        "libavfilter",
        "libavformat",
        "libavutil",
        "libswresample",
        "libswscale",
    ];

    // Explicit type annotation so the compiler knows we're storing PathBuf
    let mut all_includes: Vec<PathBuf> = Vec::new();

    for pkg in &pkgs {
        let lib_probe = match pkg_config::Config::new().probe(pkg) {
            Ok(info) => info,
            Err(err) => {
                eprintln!("cargo:warning=Failed to find {pkg} via pkg-config: {err}");
                return false;
            }
        };

        // Gather include paths
        for p in lib_probe.include_paths {
            if !all_includes.contains(&p) {
                all_includes.push(p);
            }
        }
    }

    // If all packages were found, compile C code with those includes:
    let mut cc_builder = cc::Build::new();
    cc_builder.file("c_src/extract_jpeg_frame.c");
    cc_builder.include("c_src");

    for inc in &all_includes {
        cc_builder.include(inc);
    }

    cc_builder.compile("extractframe");

    // Generate the bindings
    let mut bindgen_builder = bindgen::Builder::default()
        .header("c_src/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    for inc in &all_includes {
        bindgen_builder = bindgen_builder.clang_arg(format!("-I{}", inc.display()));
    }

    let bindings = bindgen_builder
        .generate()
        .expect("Failed to generate FFmpeg bindings via pkg-config");

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings");

    true
}