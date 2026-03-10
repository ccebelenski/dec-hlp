/*
 * dec_hlp.h — C-compatible FFI bindings for the dec-hlp VMS help library.
 *
 * All functions return an int32_t status code (0 = success, negative = error)
 * unless documented otherwise. Detailed error messages are available via
 * dechlp_last_error().
 *
 * Thread safety: each thread has its own last-error string. Library and
 * navigator handles themselves are not thread-safe — do not share a single
 * handle between threads without external synchronization.
 *
 * Memory ownership:
 *   - Pointers returned as "borrowed" (e.g., topic text) are valid as long as
 *     the library handle is alive. Do NOT free them.
 *   - Pointers returned as "caller-owned" (e.g., topic name, prompt) must be
 *     freed by calling dechlp_string_free().
 *   - Navigator handles must be destroyed before the library they reference.
 */

#ifndef DEC_HLP_H
#define DEC_HLP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Return codes ─────────────────────────────────────────────────────────── */

#define DECHLP_OK             0
#define DECHLP_ERR_NULL_ARG  -1
#define DECHLP_ERR_IO        -2
#define DECHLP_ERR_FORMAT    -3
#define DECHLP_ERR_NOT_FOUND -4
#define DECHLP_ERR_AMBIGUOUS -5
#define DECHLP_ERR_BUILD     -6
#define DECHLP_ERR_INVALID   -7
#define DECHLP_ERR_INTERNAL  -99

/* ── Navigator action codes (positive, returned by dechlp_navigator_input) ─ */

#define DECHLP_NAV_DISPLAY_TOPIC  1
#define DECHLP_NAV_AMBIGUOUS      2
#define DECHLP_NAV_NOT_FOUND      3
#define DECHLP_NAV_SHOW_TOPICS    4
#define DECHLP_NAV_GO_UP          5
#define DECHLP_NAV_EXIT           6
#define DECHLP_NAV_DISPLAY_MULTI  7

/* ── Match mode constants ─────────────────────────────────────────────────── */

#define DECHLP_MATCH_ABBREVIATION 0
#define DECHLP_MATCH_EXACT        1

/* ── Opaque handle types ──────────────────────────────────────────────────── */

/** Opaque handle to an opened .hlib library file. */
typedef struct DecHlpLibrary DecHlpLibrary;

/** Opaque handle to a navigator (interactive topic browser). */
typedef struct DecHlpNavigator DecHlpNavigator;

/* ── Error / string functions ─────────────────────────────────────────────── */

/**
 * Return the thread-local error message from the most recent failing call.
 *
 * The returned pointer is valid until the next FFI call on the same thread.
 * Returns an empty string if the last call succeeded.
 */
const char *dechlp_last_error(void);

/**
 * Free a string that was allocated by the FFI layer (e.g., from
 * dechlp_topic_name or dechlp_navigator_prompt).
 *
 * Passing NULL is a safe no-op.
 */
void dechlp_string_free(char *s);

/* ── Version ──────────────────────────────────────────────────────────────── */

/**
 * Return a static version string for the FFI library (e.g., "0.1.0").
 * The pointer is valid for the lifetime of the process.
 */
const char *dechlp_version(void);

/* ── Library functions ────────────────────────────────────────────────────── */

/**
 * Open a .hlib library file.
 *
 * @param path      Null-terminated path to the .hlib file.
 * @param out_lib   On success, receives the library handle.
 * @return DECHLP_OK on success; DECHLP_ERR_IO, DECHLP_ERR_FORMAT, or
 *         DECHLP_ERR_NULL_ARG on failure.
 */
int32_t dechlp_library_open(const char *path, DecHlpLibrary **out_lib);

/**
 * Close (destroy) a library handle. Passing NULL is a safe no-op.
 *
 * All navigators created from this library must be destroyed first.
 */
void dechlp_library_close(DecHlpLibrary *lib);

/**
 * Return the number of nodes in the library. Returns 0 if lib is NULL.
 */
uint32_t dechlp_library_node_count(const DecHlpLibrary *lib);

/**
 * Return the build timestamp (seconds since Unix epoch) from the library
 * header. Returns 0 if lib is NULL.
 */
uint64_t dechlp_library_build_timestamp(const DecHlpLibrary *lib);

/* ── Topic lookup functions ───────────────────────────────────────────────── */

/**
 * Look up a topic by path and return a pointer to its body text.
 *
 * @param lib           Library handle.
 * @param path          Array of path_len null-terminated C strings.
 * @param path_len      Number of path components.
 * @param match_mode    DECHLP_MATCH_ABBREVIATION or DECHLP_MATCH_EXACT.
 * @param out_text      On success, receives a pointer to the body text.
 *                      This is borrowed from the library and valid as long as
 *                      the library handle is alive. Do NOT free it.
 * @param out_text_len  On success, receives the byte length of the body text.
 * @return DECHLP_OK, DECHLP_ERR_NOT_FOUND, or DECHLP_ERR_AMBIGUOUS.
 */
int32_t dechlp_topic_lookup(const DecHlpLibrary *lib,
                            const char *const *path,
                            size_t path_len,
                            int32_t match_mode,
                            const char **out_text,
                            size_t *out_text_len);

/**
 * Look up a topic by path and return its display name.
 *
 * @param lib           Library handle.
 * @param path          Array of path_len null-terminated C strings.
 * @param path_len      Number of path components.
 * @param match_mode    DECHLP_MATCH_ABBREVIATION or DECHLP_MATCH_EXACT.
 * @param out_name      On success, receives a caller-owned string.
 *                      Free with dechlp_string_free().
 * @return DECHLP_OK, DECHLP_ERR_NOT_FOUND, or DECHLP_ERR_AMBIGUOUS.
 */
int32_t dechlp_topic_name(const DecHlpLibrary *lib,
                          const char *const *path,
                          size_t path_len,
                          int32_t match_mode,
                          char **out_name);

/**
 * Return the number of children of the topic at the given path.
 *
 * Pass path=NULL and path_len=0 to query the root node's children.
 *
 * @param out_count  On success, receives the child count.
 */
int32_t dechlp_children_count(const DecHlpLibrary *lib,
                              const char *const *path,
                              size_t path_len,
                              int32_t match_mode,
                              uint32_t *out_count);

/**
 * Return the name of child at `index` of the topic at the given path.
 *
 * @param index     Zero-based child index.
 * @param out_name  On success, receives a caller-owned string.
 *                  Free with dechlp_string_free().
 */
int32_t dechlp_children_name(const DecHlpLibrary *lib,
                             const char *const *path,
                             size_t path_len,
                             int32_t match_mode,
                             uint32_t index,
                             char **out_name);

/**
 * Return all children names as a null-separated string.
 *
 * On success, *out_names is a caller-owned buffer containing each name
 * separated by '\0', with a final '\0'. *out_total_len is the total byte
 * length including all separators.
 *
 * Example for children ["COPY", "DELETE"]:
 *   buffer = "COPY\0DELETE\0", total_len = 12
 *
 * Free *out_names with dechlp_string_free().
 */
int32_t dechlp_children_names(const DecHlpLibrary *lib,
                              const char *const *path,
                              size_t path_len,
                              int32_t match_mode,
                              char **out_names,
                              size_t *out_total_len);

/* ── Build ────────────────────────────────────────────────────────────────── */

/**
 * Build a .hlib file from one or more .hlp source files.
 *
 * @param input_paths   Array of input_count null-terminated paths to .hlp files.
 * @param input_count   Number of input files (must be >= 1).
 * @param output_path   Null-terminated path for the output .hlib file.
 * @return DECHLP_OK or DECHLP_ERR_BUILD.
 */
int32_t dechlp_build(const char *const *input_paths,
                     size_t input_count,
                     const char *output_path);

/* ── Navigator functions ──────────────────────────────────────────────────── */

/**
 * Create a navigator for interactive topic browsing.
 *
 * The library handle MUST outlive the navigator. Destroying the library while
 * a navigator referencing it still exists is undefined behavior.
 *
 * @param lib       Library handle (must remain valid).
 * @param out_nav   On success, receives the navigator handle.
 */
int32_t dechlp_navigator_create(const DecHlpLibrary *lib,
                                DecHlpNavigator **out_nav);

/**
 * Destroy a navigator handle. Passing NULL is a safe no-op.
 */
void dechlp_navigator_destroy(DecHlpNavigator *nav);

/**
 * Return the current depth of the navigator (0 = root). Returns 0 if NULL.
 */
size_t dechlp_navigator_depth(const DecHlpNavigator *nav);

/**
 * Get the prompt string for the current navigator position.
 *
 * @param out_prompt  On success, receives a caller-owned string.
 *                    Free with dechlp_string_free().
 */
int32_t dechlp_navigator_prompt(const DecHlpNavigator *nav, char **out_prompt);

/**
 * Process user input in the navigator.
 *
 * Returns a positive DECHLP_NAV_* action code indicating what the caller
 * should do next, or a negative error code on failure.
 *
 * @param nav         Navigator handle.
 * @param input       Null-terminated user input string.
 * @param match_mode  DECHLP_MATCH_ABBREVIATION or DECHLP_MATCH_EXACT.
 * @return One of the DECHLP_NAV_* constants, or a negative error code.
 */
int32_t dechlp_navigator_input(DecHlpNavigator *nav,
                               const char *input,
                               int32_t match_mode);

/**
 * Get the body text of the topic at the navigator's current position.
 *
 * @param out_text      Receives a borrowed pointer to the body text (valid as
 *                      long as the library handle is alive). Do NOT free it.
 * @param out_text_len  Receives the byte length of the body text.
 */
int32_t dechlp_navigator_topic_text(const DecHlpNavigator *nav,
                                    const char **out_text,
                                    size_t *out_text_len);

/**
 * Get the children names of the current navigator topic as a null-separated
 * string.
 *
 * @param out_names      Receives a caller-owned buffer. Free with
 *                       dechlp_string_free().
 * @param out_total_len  Receives the total byte length of the buffer.
 */
int32_t dechlp_navigator_topic_children(const DecHlpNavigator *nav,
                                        char **out_names,
                                        size_t *out_total_len);

/**
 * Go up one level in the navigator.
 *
 * @return DECHLP_OK on success, DECHLP_ERR_INVALID if already at root.
 */
int32_t dechlp_navigator_go_up(DecHlpNavigator *nav);

/**
 * Reset the navigator to the root level. Passing NULL is a safe no-op.
 */
void dechlp_navigator_reset(DecHlpNavigator *nav);

#ifdef __cplusplus
}
#endif

#endif /* DEC_HLP_H */
