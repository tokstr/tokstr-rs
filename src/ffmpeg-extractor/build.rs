use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // We will eventually store the C flags (include paths, etc.) we want to pass to cc and bindgen:
    let mut cflags: Vec<String> = vec![];

    // Are we cross-compiling for Android?
    let is_android = target.contains("android");

    if is_android {
        // ------------------------------------------------------
        // 1) ANDROID: Use precompiled static FFmpeg libraries
        // ------------------------------------------------------

        // Map Rust target -> FFmpeg-libs subfolder
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

        // Check if the user specified a custom path in the env var
        let ffmpeg_base = match env::var("FFMPEG_LIBS_PATH") {
            Ok(path) => PathBuf::from(path),
            Err(_) => {
                // Fallback to local "ffmpeg-libs" if not set
                PathBuf::from("ffmpeg-libs")
            }
        };

        let ffmpeg_lib_dir = ffmpeg_base.join(arch_subdir);

        // Instruct Cargo to look in that directory for static libraries
        println!("cargo:rustc-link-search=native={}", ffmpeg_lib_dir.display());

        // Statically link to each relevant FFmpeg component (add/remove as needed):
        println!("cargo:rustc-link-lib=static=avformat");
        println!("cargo:rustc-link-lib=static=avcodec");
        println!("cargo:rustc-link-lib=static=avutil");
        println!("cargo:rustc-link-lib=static=swscale");

        // If FFmpeg depends on other libraries (e.g., zlib, x264, etc.), add them:
        // println!("cargo:rustc-link-lib=static=x264");
        // println!("cargo:rustc-link-lib=static=z");

        // For Android, you typically need to link the "log" library:
        println!("cargo:rustc-link-lib=log");

        // If you have custom include paths for FFmpeg headers, you can add:
        // cflags.push("-I/path/to/ffmpeg/include".to_string());

    } else {
        // ------------------------------------------------------
        // 2) NON-ANDROID (e.g., Desktop) -> Use pkg-config
        // ------------------------------------------------------

        // Attempt to read from pkg-config, ignoring errors if not found
        let ffmpeg_cflags = Command::new("pkg-config")
            .args(&["--cflags", "libavformat", "libavcodec", "libavutil", "libswscale"])
            .output()
            .ok()
            .map(|out| String::from_utf8(out.stdout).unwrap())
            .unwrap_or_default();

        let ffmpeg_libs = Command::new("pkg-config")
            .args(&["--libs", "libavformat", "libavcodec", "libavutil", "libswscale"])
            .output()
            .ok()
            .map(|out| String::from_utf8(out.stdout).unwrap())
            .unwrap_or_default();

        // Add cflags to our vector
        for flag in ffmpeg_cflags.split_whitespace() {
            cflags.push(flag.to_string());
        }

        // Instruct Cargo how to link
        for lib_arg in ffmpeg_libs.split_whitespace() {
            if lib_arg.starts_with("-l") {
                // e.g. "-lavcodec" -> "avcodec"
                println!("cargo:rustc-link-lib={}", &lib_arg[2..]);
            } else if lib_arg.starts_with("-L") {
                // e.g. "-L/some/dir"
                println!("cargo:rustc-link-search=native={}", &lib_arg[2..]);
            }
        }
    }

    // ------------------------------------------------------
    // 3) Compile your C code
    // ------------------------------------------------------

    let mut build = cc::Build::new();
    build
        .file("c_src/extract_jpeg_frame.c")
        .include("c_src"); // local includes

    // If you have additional `.c` files, add them here:
    // build.file("c_src/another_file.c");

    // Add each cflag individually
    for cflag in &cflags {
        build.flag(cflag);
    }

    // Now compile
    build.compile("extractframe");

    // ------------------------------------------------------
    // 4) Run bindgen to generate Rust FFI
    // ------------------------------------------------------
    // We pass along any cflags that matter for finding FFmpeg headers, etc.
    let bindings = bindgen::Builder::default()
        .header("c_src/wrapper.h")
        .clang_args(&cflags)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings from wrapper.h");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}