/*
 * Example: List all topics in a .hlib library using the C FFI bindings.
 *
 * Build:
 *   # First build the FFI library:
 *   cargo build -p dec-hlp-ffi --release
 *
 *   # Then compile this example:
 *   gcc -o list_topics list_topics.c \
 *       -I../../dec-hlp-ffi/include \
 *       -L../../target/release \
 *       -ldec_hlp_ffi -lpthread -ldl -lm
 *
 *   # Run with LD_LIBRARY_PATH if using the shared library:
 *   LD_LIBRARY_PATH=../../target/release ./list_topics path/to/library.hlib
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dec_hlp.h"

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <library.hlib>\n", argv[0]);
        return 1;
    }

    /* Open the library */
    DecHlpLibrary *lib = NULL;
    int32_t rc = dechlp_library_open(argv[1], &lib);
    if (rc != DECHLP_OK) {
        const char *err = dechlp_last_error();
        fprintf(stderr, "Error opening library: %s\n", err ? err : "unknown error");
        return 1;
    }

    /* Print library info */
    printf("Library: %s\n", argv[1]);
    printf("Nodes: %u\n", dechlp_library_node_count(lib));
    printf("\n");

    /* List all root topics */
    size_t count = 0;
    rc = dechlp_children_count(lib, NULL, 0, DECHLP_MATCH_ABBREVIATION, &count);
    if (rc != DECHLP_OK) {
        fprintf(stderr, "Error counting topics\n");
        dechlp_library_close(lib);
        return 1;
    }

    for (size_t i = 0; i < count; i++) {
        char *name = NULL;
        rc = dechlp_children_name(lib, NULL, 0, DECHLP_MATCH_ABBREVIATION, i, &name);
        if (rc == DECHLP_OK && name) {
            printf("%s\n", name);

            /* List subtopics of this topic */
            const char *path[1] = { name };
            size_t sub_count = 0;
            int32_t src = dechlp_children_count(lib, path, 1, DECHLP_MATCH_EXACT, &sub_count);
            if (src == DECHLP_OK && sub_count > 0) {
                for (size_t j = 0; j < sub_count; j++) {
                    char *sub_name = NULL;
                    int32_t snr = dechlp_children_name(lib, path, 1, DECHLP_MATCH_EXACT, j, &sub_name);
                    if (snr == DECHLP_OK && sub_name) {
                        printf("  %s\n", sub_name);
                        dechlp_string_free(sub_name);
                    }
                }
            }

            dechlp_string_free(name);
        }
    }

    /* Clean up */
    dechlp_library_close(lib);
    return 0;
}
