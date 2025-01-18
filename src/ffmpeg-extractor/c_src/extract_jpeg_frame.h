#ifndef EXTRACT_JPEG_FRAME_H
#define EXTRACT_JPEG_FRAME_H

#include <stddef.h>
#include <stdint.h>

/**
 * A simple container to hold the encoded JPEG in memory.
 */
typedef struct {
    uint8_t* frameData;   // Pointer to JPEG-encoded bytes
    int frameSize;        // Number of bytes in frameData
} FrameData;

/**
 * Extract the *first* video frame from videoData, encode it as JPEG in memory,
 * and return a FrameData struct containing the JPEG bytes and size.
 *
 * Return NULL on error (e.g., if no valid frame can be decoded).
 */
FrameData* extract_jpeg_frame(const uint8_t* videoData, size_t dataSize);

/**
 * Free a FrameData struct allocated by extract_jpeg_frame().
 */
void free_frame_data(FrameData* frame);

#endif
