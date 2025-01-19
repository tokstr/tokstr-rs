use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Where your precompiled FFmpeg is located, e.g. "3rd-party/ffmpeg-libs/"
    // If not set, defaults to "3rd-party/ffmpeg-libs".
    let ffmpeg_libs_path = env::var("FFMPEG_LIBS_PATH")
        .unwrap_or_else(|_| "3rd-party/ffmpeg-libs".to_string());

    // Determine if we're on Android by checking the target triple
    let is_android = target.contains("android");
    // We may or may not want to handle non-Android separately
    // but here we assume you're primarily targeting Android.

    if is_android {
        // Map the Rust target triple to the correct FFmpeg subfolder
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

        let ffmpeg_include_dir = PathBuf::from(&ffmpeg_libs_path).join(arch_subdir).join("include");
        let ffmpeg_lib_dir = PathBuf::from(&ffmpeg_libs_path).join(arch_subdir).join("lib");

        // Tell Cargo (rustc) where to find the .a files
        println!("cargo:rustc-link-search=native={}", ffmpeg_lib_dir.display());

        // Link all the FFmpeg libraries you need, statically
        // (Adjust as needed if you don't need all)
        println!("cargo:rustc-link-lib=static=avcodec");
        println!("cargo:rustc-link-lib=static=avdevice");
        println!("cargo:rustc-link-lib=static=avfilter");
        println!("cargo:rustc-link-lib=static=avformat");
        println!("cargo:rustc-link-lib=static=avutil");
        println!("cargo:rustc-link-lib=static=postproc");
        println!("cargo:rustc-link-lib=static=swresample");
        println!("cargo:rustc-link-lib=static=swscale");

        // Usually required on Android to link the system log library
        println!("cargo:rustc-link-lib=log");

        // ------------------------------------------------
        //  Compile your C code for Android
        // ------------------------------------------------
        let mut build = cc::Build::new();
        build
            .file("c_src/extract_jpeg_frame.c")
            // Add the path to the FFmpeg "include" directory
            .include("c_src")
            .include(ffmpeg_include_dir.clone())  // important so <libavcodec/avcodec.h> is found
        ;

        // If you need extra flags, e.g. build.flag("-DWHATEVER");
        // build.flag("-DANDROID");

        build.compile("extractframe");

        // ------------------------------------------------
        //  Run bindgen to generate Rust FFI
        // ------------------------------------------------
        let mut bindings = bindgen::Builder::default()
            .header("c_src/wrapper.h")
            .clang_args(&[
                // Pass the -I flag to Clang so it finds <libavcodec/avcodec.h> in that include dir
                format!("-I{}", ffmpeg_include_dir.display()),
            ])
            // This ensures Cargo rebuilds if files in c_src change
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        let bindings = bindings
            .generate()
            .expect("Unable to generate FFmpeg bindings via bindgen");

        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("Couldn't write bindings!");
    } else {
        // If you do want to handle non-Android (e.g. desktop) with pkg-config:
        // (Optional section - remove if irrelevant)
        fallback_pkg_config_build();
    }
}

fn fallback_pkg_config_build() {
    // Example: we just do something naive with pkg-config.
    let ffmpeg_cflags = Command::new("pkg-config")
        .args(["--cflags", "libavformat", "libavcodec", "libavutil", "libswscale"])
        .output()
        .ok()
        .map(|out| String::from_utf8(out.stdout).unwrap())
        .unwrap_or_default();

    let ffmpeg_libs = Command::new("pkg-config")
        .args(["--libs", "libavformat", "libavcodec", "libavutil", "libswscale"])
        .output()
        .ok()
        .map(|out| String::from_utf8(out.stdout).unwrap())
        .unwrap_or_default();

    // Compile your C code (desktop scenario)
    let mut build = cc::Build::new();
    build.file("c_src/extract_jpeg_frame.c").include("c_src");

    for flag in ffmpeg_cflags.split_whitespace() {
        build.flag(flag);
    }
    build.compile("extractframe");

    // Link
    for lib_arg in ffmpeg_libs.split_whitespace() {
        if lib_arg.starts_with("-l") {
            println!("cargo:rustc-link-lib={}", &lib_arg[2..]);
        } else if lib_arg.starts_with("-L") {
            println!("cargo:rustc-link-search=native={}", &lib_arg[2..]);
        }
    }

    // bindgen
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings = bindgen::Builder::default()
        .header("c_src/wrapper.h")
        .clang_args(ffmpeg_cflags.split_whitespace())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings from wrapper.h");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
