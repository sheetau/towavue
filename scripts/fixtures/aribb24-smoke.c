#include <stdbool.h>
#include <stddef.h>
#include <time.h>
#include <aribb24/aribb24.h>
#include <aribb24/decoder.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    arib_instance_t *instance = arib_instance_new(NULL);
    if (instance == NULL) {
        fputs("ARIB instance allocation failed.\n", stderr);
        return 1;
    }
    arib_decoder_t *decoder = arib_get_decoder(instance);
    if (decoder == NULL) {
        arib_instance_destroy(instance);
        fputs("ARIB decoder allocation failed.\n", stderr);
        return 1;
    }
    arib_initialize_decoder_a_profile(decoder);
    /* LS1 selects the initial alphanumeric G1 set; A maps to U+FF21. */
    const unsigned char input[] = {0x0e, 0x41};
    const char expected[] = "\xef\xbc\xa1";
    char output[32] = {0};
    size_t count = arib_decode_buffer(decoder, input, sizeof(input), output, sizeof(output));
    int matched = count == sizeof(expected) - 1 && memcmp(output, expected, count) == 0;
    arib_finalize_decoder(decoder);
    arib_instance_destroy(instance);
    if (!matched) {
        fprintf(stderr, "ARIB alphanumeric decode mismatch (%zu bytes).\n", count);
        return 1;
    }
    puts("Native ARIB C API decoded LS1/A to UTF-8 U+FF21.");
    return 0;
}
