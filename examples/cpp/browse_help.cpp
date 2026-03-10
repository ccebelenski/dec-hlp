/*
 * Example: Interactive help browser using the C++ FFI bindings.
 *
 * This demonstrates using the Navigator API from C++ with RAII wrappers.
 *
 * Build:
 *   cargo build -p dec-hlp-ffi --release
 *   g++ -std=c++17 -o browse_help browse_help.cpp \
 *       -I../../dec-hlp-ffi/include \
 *       -L../../target/release \
 *       -ldec_hlp_ffi -lpthread -ldl -lm
 *
 * Usage:
 *   LD_LIBRARY_PATH=../../target/release ./browse_help library.hlib
 */

#include <iostream>
#include <string>
#include <vector>
#include <memory>
#include <cstring>
#include "dec_hlp.h"

// RAII wrappers for dec-hlp handles

struct LibraryDeleter {
    void operator()(DecHlpLibrary *lib) const {
        dechlp_library_close(lib);
    }
};

struct NavigatorDeleter {
    void operator()(DecHlpNavigator *nav) const {
        dechlp_navigator_destroy(nav);
    }
};

struct StringDeleter {
    void operator()(char *s) const {
        dechlp_string_free(s);
    }
};

using LibraryPtr = std::unique_ptr<DecHlpLibrary, LibraryDeleter>;
using NavigatorPtr = std::unique_ptr<DecHlpNavigator, NavigatorDeleter>;
using CString = std::unique_ptr<char, StringDeleter>;

// Parse a null-separated string into a vector
std::vector<std::string> parse_null_separated(const char *data, size_t len) {
    std::vector<std::string> result;
    if (!data || len == 0) return result;
    const char *p = data;
    while (*p) {
        result.emplace_back(p);
        p += std::strlen(p) + 1;
    }
    return result;
}

int main(int argc, char *argv[]) {
    if (argc != 2) {
        std::cerr << "Usage: " << argv[0] << " <library.hlib>" << std::endl;
        return 1;
    }

    // Open library with RAII
    DecHlpLibrary *raw_lib = nullptr;
    int32_t rc = dechlp_library_open(argv[1], &raw_lib);
    if (rc != DECHLP_OK) {
        std::cerr << "Error: " << (dechlp_last_error() ? dechlp_last_error() : "unknown")
                  << std::endl;
        return 1;
    }
    LibraryPtr lib(raw_lib);

    std::cout << "Library: " << argv[1] << std::endl;
    std::cout << "Nodes: " << dechlp_library_node_count(lib.get()) << std::endl;
    std::cout << "Version: " << dechlp_version() << std::endl;
    std::cout << std::endl;

    // Create navigator with RAII
    DecHlpNavigator *raw_nav = nullptr;
    rc = dechlp_navigator_create(lib.get(), &raw_nav);
    if (rc != DECHLP_OK) {
        std::cerr << "Failed to create navigator" << std::endl;
        return 1;
    }
    NavigatorPtr nav(raw_nav);

    // Show initial topics
    {
        char *names = nullptr;
        size_t names_len = 0;
        rc = dechlp_navigator_topic_children(nav.get(), &names, &names_len);
        if (rc == DECHLP_OK && names) {
            auto topics = parse_null_separated(names, names_len);
            std::cout << "  Available topics:" << std::endl << std::endl;
            for (const auto &t : topics) {
                std::cout << "  " << t << std::endl;
            }
            std::cout << std::endl;
            dechlp_string_free(names);
        }
    }

    // Interactive loop
    while (true) {
        // Get and display prompt
        char *raw_prompt = nullptr;
        dechlp_navigator_prompt(nav.get(), &raw_prompt);
        CString prompt(raw_prompt);
        std::cerr << prompt.get();

        // Read input
        std::string line;
        if (!std::getline(std::cin, line)) {
            break;  // EOF
        }

        rc = dechlp_navigator_input(nav.get(), line.c_str(), DECHLP_MATCH_ABBREVIATION);

        switch (rc) {
            case DECHLP_NAV_DISPLAY_TOPIC: {
                // Get topic text
                const char *text = nullptr;
                size_t text_len = 0;
                dechlp_navigator_topic_text(nav.get(), &text, &text_len);
                if (text && text_len > 0) {
                    std::cout << std::endl;
                    std::cout.write(text, text_len);
                    if (text[text_len - 1] != '\n') std::cout << std::endl;
                }

                // Show children
                char *children = nullptr;
                size_t children_len = 0;
                dechlp_navigator_topic_children(nav.get(), &children, &children_len);
                if (children && children_len > 0) {
                    auto subtopics = parse_null_separated(children, children_len);
                    if (!subtopics.empty()) {
                        std::cout << std::endl;
                        std::cout << "  Additional information available:" << std::endl;
                        std::cout << std::endl;
                        for (const auto &s : subtopics) {
                            std::cout << "  " << s << std::endl;
                        }
                        std::cout << std::endl;
                    }
                    dechlp_string_free(children);
                }
                break;
            }

            case DECHLP_NAV_AMBIGUOUS:
                std::cerr << std::endl << "  Topic is ambiguous. "
                          << (dechlp_last_error() ? dechlp_last_error() : "")
                          << std::endl << std::endl;
                break;

            case DECHLP_NAV_NOT_FOUND:
                std::cerr << std::endl << "  "
                          << (dechlp_last_error() ? dechlp_last_error() : "Not found")
                          << std::endl << std::endl;
                break;

            case DECHLP_NAV_SHOW_TOPICS: {
                char *children = nullptr;
                size_t children_len = 0;
                dechlp_navigator_topic_children(nav.get(), &children, &children_len);
                if (children) {
                    auto topics = parse_null_separated(children, children_len);
                    std::cout << std::endl;
                    for (const auto &t : topics) {
                        std::cout << "  " << t << std::endl;
                    }
                    std::cout << std::endl;
                    dechlp_string_free(children);
                }
                break;
            }

            case DECHLP_NAV_GO_UP:
                break;

            case DECHLP_NAV_EXIT:
                return 0;

            default:
                break;
        }
    }

    return 0;
}
