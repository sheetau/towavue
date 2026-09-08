#include <lilv/lilv.h>
#include <lv2/core/lv2.h>
#include <va/va.h>
#include <stdio.h>

int main(void) {
    LilvWorld* world = lilv_world_new();
    if (!world) return 1;
    LilvNode* plugin = lilv_new_uri(world, LV2_CORE__Plugin);
    int valid = plugin && lilv_node_is_uri(plugin);
    lilv_node_free(plugin);
    lilv_world_free(world);
    const char* description = vaErrorStr(VA_STATUS_SUCCESS);
    if (!valid || !description || !description[0]) return 1;
    puts("LV2/lilv and VAAPI compile/link/runtime smoke passed; no plugins or hardware initialized.");
    return 0;
}
