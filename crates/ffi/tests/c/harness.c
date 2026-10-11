/* C-side check of the engine-ffi ABI, the way the native Photoshop shim will call it.
 * Build and run from the repo root (see IMAGE_ENGINE_VALIDATION.md):
 *   cargo build -p engine-ffi
 *   clang crates/ffi/tests/c/harness.c target/debug/libengine_ffi.a -o target/ffi_harness
 *   ./target/ffi_harness
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int32_t pe_apply_exposure_rgba8(uint8_t *pixels, uint32_t width, uint32_t height,
                                size_t row_bytes, float ev);

static int failures = 0;

#define CHECK(cond, msg)                                   \
    do {                                                   \
        if (!(cond)) {                                     \
            printf("FAIL: %s (line %d)\n", msg, __LINE__); \
            failures++;                                    \
        } else {                                           \
            printf("ok:   %s\n", msg);                     \
        }                                                  \
    } while (0)

/* 2x2 RGBA, 12-byte rows: 8 bytes of pixels then 4 bytes of 0xAA padding per row. */
static void fill(uint8_t *buf) {
    memset(buf, 0xAA, 24);
    for (int y = 0; y < 2; y++) {
        for (int x = 0; x < 2; x++) {
            uint8_t *p = buf + y * 12 + x * 4;
            p[0] = 100; p[1] = 100; p[2] = 100; p[3] = 255;
        }
    }
}

int main(void) {
    uint8_t buf[24], before[24];

    fill(buf);
    memcpy(before, buf, 24);
    CHECK(pe_apply_exposure_rgba8(buf, 2, 2, 12, 0.0f) == 0, "ev 0 returns OK");
    CHECK(memcmp(buf, before, 24) == 0, "ev 0 leaves pixels and padding identical");

    fill(buf);
    CHECK(pe_apply_exposure_rgba8(buf, 2, 2, 12, 1.0f) == 0, "ev +1 returns OK");
    CHECK(buf[0] > 100 && buf[12] > 100, "ev +1 brightens red in both rows");
    CHECK(buf[8] == 0xAA && buf[9] == 0xAA && buf[10] == 0xAA && buf[11] == 0xAA,
          "ev +1 keeps row 0 padding");
    CHECK(buf[20] == 0xAA && buf[23] == 0xAA, "ev +1 keeps row 1 padding");

    fill(buf);
    memcpy(before, buf, 24);
    CHECK(pe_apply_exposure_rgba8(NULL, 2, 2, 12, 0.0f) == -1, "null pointer returns -1");
    CHECK(pe_apply_exposure_rgba8(buf, 2, 2, 7, 0.0f) == -3, "rowbytes below width*4 returns -3");
    CHECK(pe_apply_exposure_rgba8(buf, 0, 2, 12, 0.0f) == -2, "zero width returns -2");
    CHECK(memcmp(buf, before, 24) == 0, "rejected calls leave buffer untouched");

    if (failures) {
        printf("%d check(s) failed\n", failures);
        return 1;
    }
    printf("all ffi checks passed\n");
    return 0;
}
