#include <librist/librist.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    struct rist_ctx *context = NULL;
    if (strcmp(librist_version(), "4f45ef8") != 0) {
        fprintf(stderr, "Unexpected libRIST version: %s\n", librist_version());
        return 1;
    }
    if (rist_receiver_create(&context, RIST_PROFILE_MAIN, NULL) != 0 || context == NULL) {
        fputs("RIST receiver creation failed.\n", stderr);
        return 1;
    }
    int result = rist_destroy(context);
    if (result != 0) {
        fprintf(stderr, "RIST receiver destruction failed: %d\n", result);
        return 1;
    }
    puts("Native RIST receiver creation/destruction passed; no peer or stream started.");
    return 0;
}
