use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Pull the env var (or default if not set)
    let ffmpeg_libs_path_raw =
        env::var("FFMPEG_LIBS_PATH").unwrap_or_else(|_| "3rd-party/ffmpeg-libs".to_string());

    // Convert to a PathBuf
    let ffmpeg_libs_path = PathBuf::from(ffmpeg_libs_path_raw);

    // Attempt to canonicalize -> absolute path
    // (If it doesn't exist, this will error out, so you may want to handle that more gracefully.)
    let ffmpeg_libs_path = ffmpeg_libs_path
        .canonicalize()
        .expect("Could not canonicalize FFMPEG_LIBS_PATH");

    // Print it out for debugging
    println!("cargo:warning=Using FFMPEG_LIBS_PATH = {}", ffmpeg_libs_path.display());

    // ...
    // Then do your usual is_android logic:
    if target.contains("android") {
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

        let ffmpeg_include_dir = ffmpeg_libs_path.join(arch_subdir).join("include");
        let ffmpeg_lib_dir = ffmpeg_libs_path.join(arch_subdir).join("lib");

        // Double check these actually exist
        if !ffmpeg_include_dir.is_dir() {
            panic!("FFmpeg include dir not found: {}", ffmpeg_include_dir.display());
        }
        if !ffmpeg_lib_dir.is_dir() {
            panic!("FFmpeg lib dir not found: {}", ffmpeg_lib_dir.display());
        }

        // Link .a files
        println!("cargo:rustc-link-search=native={}", ffmpeg_lib_dir.display());
        println!("cargo:rustc-link-lib=static=avcodec");
        println!("cargo:rustc-link-lib=static=avdevice");
        println!("cargo:rustc-link-lib=static=avfilter");
        println!("cargo:rustc-link-lib=static=avformat");
        println!("cargo:rustc-link-lib=static=avutil");
        println!("cargo:rustc-link-lib=static=postproc");
        println!("cargo:rustc-link-lib=static=swresample");
        println!("cargo:rustc-link-lib=static=swscale");
        println!("cargo:rustc-link-lib=log");

        // Compile your .c code
        let mut cc_builder = cc::Build::new();
        cc_builder
            .file("c_src/extract_jpeg_frame.c")
            .include("c_src")
            .include(&ffmpeg_include_dir); // important

        cc_builder.compile("extractframe");

        // Bindgen
        let mut bindgen_builder = bindgen::Builder::default()
            .header("c_src/wrapper.h")
            .clang_args(&[format!("-I{}", ffmpeg_include_dir.display())])
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        let bindings = bindgen_builder
            .generate()
            .expect("Failed to generate FFmpeg bindings");

        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("Couldn't write bindings");
    } else {
        // Non-Android fallback, if needed
    }
}
