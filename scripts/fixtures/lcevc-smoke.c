#include <LCEVC/lcevc_dec.h>
#include <stdio.h>

int main(void) {
    LCEVC_DecoderHandle decoder = {0};
    LCEVC_AccelContextHandle acceleration = {0};
    LCEVC_ReturnCode result = LCEVC_CreateDecoder(&decoder, acceleration);
    if (result != LCEVC_Success) {
        fprintf(stderr, "LCEVC_CreateDecoder failed: %d\n", result);
        return 1;
    }
    result = LCEVC_InitializeDecoder(decoder);
    LCEVC_DestroyDecoder(decoder);
    if (result != LCEVC_Success) {
        fprintf(stderr, "LCEVC_InitializeDecoder failed: %d\n", result);
        return 1;
    }
    puts("Native LCEVC C API initialization and destruction passed.");
    return 0;
}
