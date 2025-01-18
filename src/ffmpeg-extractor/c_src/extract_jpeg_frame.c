#include "extract_jpeg_frame.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libswscale/swscale.h>
#include <libavutil/imgutils.h>
#include <libavutil/mem.h>


/**
 * Free a FrameData struct allocated by extract_jpeg_frame().
 */
void free_frame_data(FrameData* frame) {
    if (!frame) return;
    if (frame->frameData) {
        free(frame->frameData);
    }
    free(frame);
}

/**
 * A small struct for our custom in-memory I/O.
 */
typedef struct {
    const uint8_t* buffer;   // The entire video data buffer
    size_t size;             // Size of that buffer
    size_t position;         // Current reading position
} IOContext;

/**
 * Callback to read data from our in-memory buffer (FFmpeg calls this).
 */
static int read_packet(void* opaque, uint8_t* buf, int buf_size) {
    IOContext* ioCtx = (IOContext*)opaque;
    int remaining = (int)(ioCtx->size - ioCtx->position);
    if (remaining <= 0) {
        // No more data
        return AVERROR_EOF;
    }
    int to_read = buf_size < remaining ? buf_size : remaining;
    memcpy(buf, ioCtx->buffer + ioCtx->position, to_read);
    ioCtx->position += to_read;
    return to_read;
}

/**
 * Extract the *first* video frame from videoData, encode it as JPEG in memory,
 * and return a FrameData struct containing the JPEG bytes and size.
 *
 * Return NULL on error (e.g., if no valid frame can be decoded).
 */
FrameData* extract_jpeg_frame(const uint8_t* videoData, size_t dataSize) {
    // Allocate an IO buffer for FFmpeg to read from
    const int ioBufferSize = 32 * 1024; // 32k
    unsigned char* ioBuffer = NULL;
    AVIOContext* avioCtx = NULL;
    AVFormatContext* formatCtx = NULL;
    AVCodecContext* decoderCtx = NULL;
    AVCodecContext* encoderCtx = NULL;
    AVFrame* decodedFrame = NULL;
    AVFrame* yuvFrame = NULL;
    AVPacket* packet = NULL;
    AVPacket* encodedPacket = NULL;
    struct SwsContext* swsCtx = NULL;
    IOContext customIO = { videoData, dataSize, 0 };
    FrameData* result = NULL;

    int ret = 0;
    int videoStreamIndex = -1;

    // -------- Initialize basic FFmpeg structures ----------
    // (In modern FFmpeg, av_register_all() isn't needed.)

    // Allocate the read buffer
    ioBuffer = (unsigned char*)av_malloc(ioBufferSize);
    if (!ioBuffer) {
        fprintf(stderr, "Failed to allocate ioBuffer\n");
        goto cleanup;
    }

    // Create a custom AVIOContext to feed the data
    avioCtx = avio_alloc_context(
        ioBuffer,           // internal buffer
        ioBufferSize,       // internal buffer size
        0,                  // write_flag (0 means read-only)
        &customIO,          // user "opaque" data
        read_packet,        // read callback
        NULL,               // write callback (not used)
        NULL                // seek callback (can be NULL)
    );
    if (!avioCtx) {
        fprintf(stderr, "Failed to create avio context\n");
        goto cleanup;
    }

    // Allocate the format context
    formatCtx = avformat_alloc_context();
    if (!formatCtx) {
        fprintf(stderr, "Failed to allocate format context\n");
        goto cleanup;
    }
    formatCtx->pb = avioCtx;

    // -------- Open input from our custom IO ---------------
    ret = avformat_open_input(&formatCtx, NULL, NULL, NULL);
    if (ret < 0) {
        fprintf(stderr, "avformat_open_input() failed: %d\n", ret);
        goto cleanup;
    }

    // Read stream info (find audio/video streams, etc.)
    ret = avformat_find_stream_info(formatCtx, NULL);
    if (ret < 0) {
        fprintf(stderr, "avformat_find_stream_info() failed: %d\n", ret);
        goto cleanup;
    }

    // Find the first video stream
    for (unsigned int i = 0; i < formatCtx->nb_streams; i++) {
        if (formatCtx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            videoStreamIndex = i;
            break;
        }
    }
    if (videoStreamIndex < 0) {
        fprintf(stderr, "No video stream found\n");
        goto cleanup;
    }

    // -------- Set up decoder (based on the video stream) ---
    {
        AVCodecParameters* codecpar = formatCtx->streams[videoStreamIndex]->codecpar;
        const AVCodec* decoder = avcodec_find_decoder(codecpar->codec_id);
        if (!decoder) {
            fprintf(stderr, "Decoder not found\n");
            goto cleanup;
        }
        decoderCtx = avcodec_alloc_context3(decoder);
        if (!decoderCtx) {
            fprintf(stderr, "Failed to allocate decoder context\n");
            goto cleanup;
        }
        ret = avcodec_parameters_to_context(decoderCtx, codecpar);
        if (ret < 0) {
            fprintf(stderr, "avcodec_parameters_to_context() failed: %d\n", ret);
            goto cleanup;
        }
        ret = avcodec_open2(decoderCtx, decoder, NULL);
        if (ret < 0) {
            fprintf(stderr, "avcodec_open2() failed: %d\n", ret);
            goto cleanup;
        }
    }

    // Prepare to read packets
    packet = av_packet_alloc();
    if (!packet) {
        fprintf(stderr, "Failed to allocate packet\n");
        goto cleanup;
    }

    decodedFrame = av_frame_alloc();
    if (!decodedFrame) {
        fprintf(stderr, "Failed to allocate frame\n");
        goto cleanup;
    }

    // -------- Read frames until we decode one video frame --
    while (av_read_frame(formatCtx, packet) >= 0) {
        if (packet->stream_index == videoStreamIndex) {
            // Send packet to decoder
            ret = avcodec_send_packet(decoderCtx, packet);
            av_packet_unref(packet);
            if (ret < 0) {
                fprintf(stderr, "avcodec_send_packet() failed: %d\n", ret);
                goto cleanup;
            }

            // Receive frame from decoder
            ret = avcodec_receive_frame(decoderCtx, decodedFrame);
            if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
                // Need more data or we reached the end
                continue;
            } else if (ret < 0) {
                fprintf(stderr, "avcodec_receive_frame() failed: %d\n", ret);
                goto cleanup;
            }

            // We got one decoded frame -> now encode it as JPEG
            break; // exit the reading loop
        } else {
            // Not our video stream, just discard
            av_packet_unref(packet);
        }
    }

    // If we never got a frame, ret will be something else:
    if (ret < 0) {
        fprintf(stderr, "No frame could be decoded.\n");
        goto cleanup;
    }

    // ---------- Set up an MJPEG encoder context -----------
    {
        const AVCodec* jpegCodec = avcodec_find_encoder(AV_CODEC_ID_MJPEG);
        if (!jpegCodec) {
            fprintf(stderr, "MJPEG encoder not found\n");
            goto cleanup;
        }

        encoderCtx = avcodec_alloc_context3(jpegCodec);
        if (!encoderCtx) {
            fprintf(stderr, "Failed to allocate MJPEG encoder context\n");
            goto cleanup;
        }

        // Set image parameters (width, height, pixel format, etc.)
        encoderCtx->pix_fmt = AV_PIX_FMT_YUVJ420P;  // common for JPEG
        encoderCtx->width   = decodedFrame->width;
        encoderCtx->height  = decodedFrame->height;
        encoderCtx->time_base = (AVRational){1, 25}; // arbitrary fps
        encoderCtx->max_b_frames = 0;                // MJPEG doesn't use B-frames
        encoderCtx->compression_level = 2;           // Some encoders use this

        // Some MJPEG encoders need a specific bit rate or quality parameter
        // to produce a good image. Example: set quality scale or bit rate.
        // encoderCtx->bit_rate = 400000; // or something suitable
        // or you can set global_quality with FF_QP2LAMBDA for a certain quality

        ret = avcodec_open2(encoderCtx, jpegCodec, NULL);
        if (ret < 0) {
            fprintf(stderr, "avcodec_open2() for MJPEG failed: %d\n", ret);
            goto cleanup;
        }
    }

    // ---------- Convert the decoded frame to YUVJ420P -----
    // Some decoders might already produce YUV420P or YUVJ420P. But let's do a
    // safe approach: create a new frame in YUVJ420P and swscale from the decoded format.
    yuvFrame = av_frame_alloc();
    if (!yuvFrame) {
        fprintf(stderr, "Failed to allocate YUV frame\n");
        goto cleanup;
    }
    yuvFrame->format = encoderCtx->pix_fmt;
    yuvFrame->width  = encoderCtx->width;
    yuvFrame->height = encoderCtx->height;

    ret = av_frame_get_buffer(yuvFrame, 32); // 32-byte alignment
    if (ret < 0) {
        fprintf(stderr, "av_frame_get_buffer() failed: %d\n", ret);
        goto cleanup;
    }

    // Create a scaling context (from decodedFrame->format to YUVJ420P)
    swsCtx = sws_getContext(
        decodedFrame->width, decodedFrame->height, (enum AVPixelFormat)decodedFrame->format,
        yuvFrame->width, yuvFrame->height, (enum AVPixelFormat)yuvFrame->format,
        SWS_BICUBIC, NULL, NULL, NULL
    );
    if (!swsCtx) {
        fprintf(stderr, "sws_getContext() failed.\n");
        goto cleanup;
    }

    // Perform the conversion
    sws_scale(swsCtx,
              (const uint8_t* const*)decodedFrame->data,
              decodedFrame->linesize,
              0,
              decodedFrame->height,
              yuvFrame->data,
              yuvFrame->linesize);

    // ----------- Encode the YUV frame as JPEG -------------
    encodedPacket = av_packet_alloc();
    if (!encodedPacket) {
        fprintf(stderr, "Failed to allocate encoded packet\n");
        goto cleanup;
    }

    // Send frame to the encoder
    ret = avcodec_send_frame(encoderCtx, yuvFrame);
    if (ret < 0) {
        fprintf(stderr, "avcodec_send_frame() failed: %d\n", ret);
        goto cleanup;
    }

    // Receive the encoded packet (JPEG) from the encoder
    ret = avcodec_receive_packet(encoderCtx, encodedPacket);
    if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
        fprintf(stderr, "No packet could be encoded.\n");
        goto cleanup;
    } else if (ret < 0) {
        fprintf(stderr, "avcodec_receive_packet() failed: %d\n", ret);
        goto cleanup;
    }

    // ---------- Copy encodedPacket->data into our result ---
    result = (FrameData*)malloc(sizeof(FrameData));
    if (!result) {
        fprintf(stderr, "Failed to allocate FrameData\n");
        goto cleanup;
    }
    result->frameSize = encodedPacket->size;
    result->frameData = (uint8_t*)malloc(result->frameSize);
    if (!result->frameData) {
        fprintf(stderr, "Failed to allocate FrameData->frameData\n");
        free(result);
        result = NULL;
        goto cleanup;
    }
    memcpy(result->frameData, encodedPacket->data, encodedPacket->size);

    // Successfully got our JPEG in memory!

cleanup:
    // -------- Clean up everything -----------
    if (encodedPacket) {
        av_packet_free(&encodedPacket);
    }
    if (yuvFrame) {
        av_frame_free(&yuvFrame);
    }
    if (swsCtx) {
        sws_freeContext(swsCtx);
        swsCtx = NULL;
    }
    if (decodedFrame) {
        av_frame_free(&decodedFrame);
    }
    if (packet) {
        av_packet_free(&packet);
    }

    if (encoderCtx) {
        avcodec_free_context(&encoderCtx);
    }
    if (decoderCtx) {
        avcodec_free_context(&decoderCtx);
    }
    if (formatCtx) {
        avformat_close_input(&formatCtx);
        avformat_free_context(formatCtx);
    }
    if (avioCtx) {
        // Note: avio_alloc_context() created the buffer in av_malloc(), so free it properly:
        if (avioCtx->buffer) {
            av_freep(&avioCtx->buffer);
        }
        av_freep(&avioCtx);
    }

    return result;
}

