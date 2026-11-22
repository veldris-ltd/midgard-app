// The weak defaults of MidgardHooks.h: nothing registered. [FR-C-1]
#include "MidgardHooks.h"

__attribute__((weak)) const void *_Nullable midgard_heimdall_3d_hook(void) {
    return 0;
}
