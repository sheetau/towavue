#include <vvenc/vvenc.h>
#include <stdio.h>

int main(void) {
    vvenc_config config;
    if (vvenc_init_default(&config, 128, 128, 30, 0, 32, VVENC_FASTER) != VVENC_OK) {
        fputs("VVenC default configuration failed.\n", stderr);
        return 1;
    }
    config.m_numThreads = 1;
    config.m_framesToBeEncoded = 1;
    vvencEncoder *encoder = vvenc_encoder_create();
    if (encoder == NULL) {
        fputs("VVenC encoder allocation failed.\n", stderr);
        return 1;
    }
    if (vvenc_encoder_open(encoder, &config) != VVENC_OK) {
        fprintf(stderr, "VVenC open failed: %s\n", vvenc_get_last_error(encoder));
        vvenc_encoder_close(encoder);
        return 1;
    }
    vvencYUVBuffer picture;
    vvenc_YUVBuffer_default(&picture);
    vvenc_YUVBuffer_alloc_buffer(&picture, VVENC_CHROMA_420, 128, 128);
    for (int plane = 0; plane < 3; ++plane) {
        if (picture.planes[plane].ptr == NULL) {
            vvenc_YUVBuffer_free_buffer(&picture);
            vvenc_encoder_close(encoder);
            fputs("VVenC picture allocation failed.\n", stderr);
            return 1;
        }
        for (int y = 0; y < picture.planes[plane].height; ++y) {
            for (int x = 0; x < picture.planes[plane].width; ++x) {
                picture.planes[plane].ptr[y * picture.planes[plane].stride + x] = 128;
            }
        }
    }
    unsigned char payload[128 * 128 * 6];
    vvencAccessUnit output;
    vvenc_accessUnit_default(&output);
    output.payload = payload;
    output.payloadSize = sizeof(payload);
    bool done = false;
    int result = VVENC_OK;
    int bytes = 0;
    int units = 0;
    for (int call = 0; call < 16 && !done; ++call) {
        result = vvenc_encode(encoder, call == 0 ? &picture : NULL, &output, &done);
        if (result != VVENC_OK) {
            fprintf(stderr, "VVenC encode failed: %s\n", vvenc_get_last_error(encoder));
            break;
        }
        bytes += output.payloadUsedSize;
        units += output.payloadUsedSize > 0;
    }
    vvenc_YUVBuffer_free_buffer(&picture);
    int closed = vvenc_encoder_close(encoder);
    if (result != VVENC_OK || closed != VVENC_OK || !done || units != 1 || bytes <= 0) {
        fprintf(stderr, "VVenC single-frame drain failed: %d units, %d bytes.\n", units, bytes);
        return 1;
    }
    printf("Native VVenC %s encoded and drained one 128x128 frame: %d bytes.\n", vvenc_get_version(), bytes);
    return 0;
}
