mod ffi {
    // The env!("OUT_DIR") is set by cargo; we do `concat!` to get the full path
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

// Re-export if you want them publicly (or keep them private).
use ffi::*;

use std::slice;

/// A safe Rust wrapper around `extract_jpeg_frame`.
pub fn extract_first_frame_to_jpeg(video_data: &[u8]) -> Result<Vec<u8>, String> {
    // Call the unsafe C function
    let ptr = unsafe { extract_jpeg_frame(video_data.as_ptr(), video_data.len()) };

    if ptr.is_null() {
        return Err("Failed to extract frame (null pointer returned)".into());
    }

    // The pointer is valid. Let's interpret the contents:
    let frame_data = unsafe { &*ptr }; // deref to get a reference to FrameData struct

    if frame_data.frameSize <= 0 || frame_data.frameData.is_null() {
        // Something is wrong
        unsafe {
            free_frame_data(ptr);
        }
        return Err("No frame data returned".into());
    }

    // Copy the JPEG bytes into a Vec<u8> for safe ownership in Rust
    let slice = unsafe { slice::from_raw_parts(frame_data.frameData, frame_data.frameSize as usize) };
    let jpeg_bytes = slice.to_vec();

    // Free the C-allocated memory
    unsafe {
        free_frame_data(ptr);
    }

    // Return the JPEG bytes
    Ok(jpeg_bytes)
}
