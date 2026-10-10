#include "mimalloc.h"

void editor_allocator_configure(void) {
    mi_option_set(mi_option_purge_delay, 0);
    mi_option_set(mi_option_arena_eager_commit, 0);
}
