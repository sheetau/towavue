#include <uavs3d.h>
#include <stdio.h>

int main(void) {
    uavs3d_cfg_t config = {0};
    config.frm_threads = 1;
    int error = 0;
    void *decoder = uavs3d_create(&config, NULL, &error);
    if (decoder == NULL) {
        fprintf(stderr, "AVS3 decoder creation failed: %d\n", error);
        return 1;
    }
    uavs3d_delete(decoder);
    puts("Native AVS3 decoder creation/destruction passed; no bitstream decoded.");
    return 0;
}
