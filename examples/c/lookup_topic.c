/*
 * Example: Look up a topic by path using the C FFI bindings.
 *
 * Build:
 *   cargo build -p dec-hlp-ffi --release
 *   gcc -o lookup_topic lookup_topic.c \
 *       -I../../dec-hlp-ffi/include \
 *       -L../../target/release \
 *       -ldec_hlp_ffi -lpthread -ldl -lm
 *
 * Usage:
 *   LD_LIBRARY_PATH=../../target/release ./lookup_topic library.hlib COPY /CONFIRM
 */

#include <stdio.h>
#include <stdlib.h>
#include "dec_hlp.h"

int main(int argc, char *argv[]) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <library.hlib> TOPIC [SUBTOPIC ...]\n", argv[0]);
        return 1;
    }

    /* Open the library */
    DecHlpLibrary *lib = NULL;
    int32_t rc = dechlp_library_open(argv[1], &lib);
    if (rc != DECHLP_OK) {
        fprintf(stderr, "Error: %s\n", dechlp_last_error());
        return 1;
    }

    /* Build the topic path from remaining arguments */
    const char **path = (const char **)&argv[2];
    size_t path_len = (size_t)(argc - 2);

    /* Look up the topic */
    const char *text = NULL;
    size_t text_len = 0;
    rc = dechlp_topic_lookup(lib, path, path_len, DECHLP_MATCH_ABBREVIATION,
                             &text, &text_len);

    if (rc == DECHLP_OK) {
        /* Get and print the display name */
        char *name = NULL;
        dechlp_topic_name(lib, path, path_len, DECHLP_MATCH_ABBREVIATION, &name);
        if (name) {
            printf("%s\n\n", name);
            dechlp_string_free(name);
        }

        /* Print the body text */
        if (text_len > 0) {
            fwrite(text, 1, text_len, stdout);
            if (text[text_len - 1] != '\n') {
                putchar('\n');
            }
        }

        /* Show children if any */
        char *children = NULL;
        size_t children_len = 0;
        rc = dechlp_children_names(lib, path, path_len, DECHLP_MATCH_ABBREVIATION,
                                   &children, &children_len);
        if (rc == DECHLP_OK && children_len > 0) {
            printf("\n  Additional information available:\n\n");
            /* Parse null-separated string */
            const char *p = children;
            while (*p) {
                printf("  %s\n", p);
                p += strlen(p) + 1;
            }
            dechlp_string_free(children);
        }
    } else if (rc == DECHLP_ERR_NOT_FOUND) {
        fprintf(stderr, "No documentation found. %s\n",
                dechlp_last_error() ? dechlp_last_error() : "");
        dechlp_library_close(lib);
        return 1;
    } else if (rc == DECHLP_ERR_AMBIGUOUS) {
        fprintf(stderr, "Topic is ambiguous. %s\n",
                dechlp_last_error() ? dechlp_last_error() : "");
        dechlp_library_close(lib);
        return 1;
    }

    dechlp_library_close(lib);
    return 0;
}
