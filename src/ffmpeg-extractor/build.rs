use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // 1. Collect FFmpeg cflags from pkg-config (or define them yourself).
    let ffmpeg_cflags = match Command::new("pkg-config")
        .args(["--cflags", "libavformat", "libavcodec", "libavutil", "libswscale"])
        .output()
    {
        Ok(output) => String::from_utf8(output.stdout).unwrap(),
        Err(_) => String::new(),
    };

    let ffmpeg_libs = match Command::new("pkg-config")
        .args(["--libs", "libavformat", "libavcodec", "libavutil", "libswscale"])
        .output()
    {
        Ok(output) => String::from_utf8(output.stdout).unwrap(),
        Err(_) => String::new(),
    };

    // 2. Compile our C code
    let mut build = cc::Build::new();
    build
        .file("c_src/extract_jpeg_frame.c")
        .include("c_src");

    // Add each cflag individually
    for cflag in ffmpeg_cflags.split_whitespace() {
        build.flag(cflag);
    }

    // Now compile
    build.compile("extractframe");

    // 3. Tell Cargo how to link the FFmpeg libs
    for lib_arg in ffmpeg_libs.split_whitespace() {
        if lib_arg.starts_with("-l") {
            // e.g. "-lavcodec" -> "avcodec"
            println!("cargo:rustc-link-lib={}", &lib_arg[2..]);
        } else if lib_arg.starts_with("-L") {
            // e.g. "-L/some/dir"
            println!("cargo:rustc-link-search=native={}", &lib_arg[2..]);
        }
    }

    // 4. Run bindgen to generate Rust FFI
    let bindings = bindgen::Builder::default()
        .header("c_src/wrapper.h")
        // Also pass the cflags to clang (for FFmpeg includes)
        .clang_args(ffmpeg_cflags.split_whitespace())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings from wrapper.h");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
