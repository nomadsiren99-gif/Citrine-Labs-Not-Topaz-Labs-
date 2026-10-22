// Round-trip and rejection checks for the Citrine settings blob. No Adobe SDK needed.
// Run with tests/run_settings_test.sh.

#include "CitrineSettings.h"

#include <cmath>
#include <cstdio>
#include <cstring>

static int failures = 0;
#define CHECK(cond, msg)                                       \
    do {                                                       \
        if (!(cond)) {                                         \
            std::printf("FAIL: %s (line %d)\n", msg, __LINE__); \
            failures++;                                        \
        } else {                                               \
            std::printf("ok:   %s\n", msg);                    \
        }                                                      \
    } while (0)

static bool SameBits(float a, float b)
{
    uint32_t x, y;
    std::memcpy(&x, &a, 4);
    std::memcpy(&y, &b, 4);
    return x == y;
}

int main(void)
{
    // Default matches the M0 fixed EV, so an empty handle changes nothing.
    CHECK(CitrineSettings{}.ev == 1.0f, "default EV is 1.0");

    // Round trip keeps exact float bits, including the sign of zero and values text would round.
    const float samples[] = {1.0f, 0.0f, -0.0f, 2.5f, -3.25f, 0.1f, kCitrineEVMax, kCitrineEVMin};
    bool exact = true;
    for (float ev : samples)
    {
        CitrineSettings in;
        in.ev = ev;
        uint8_t blob[kCitrineBlobSize];
        EncodeCitrineSettings(in, blob);
        CitrineSettings out;
        out.ev = 123.0f;
        if (DecodeCitrineSettings(blob, sizeof blob, out) != BlobStatus::Ok || !SameBits(out.ev, ev))
            exact = false;
    }
    CHECK(exact, "round trip keeps exact float32 bits for every sample");

    // Layout is what the doc and the reader expect: magic, version 1, ev little-endian.
    {
        CitrineSettings s;
        s.ev = 1.0f; // 0x3F800000
        uint8_t blob[kCitrineBlobSize];
        EncodeCitrineSettings(s, blob);
        const uint8_t expect[9] = {'C', 'T', 'R', 'N', 1, 0x00, 0x00, 0x80, 0x3F};
        CHECK(std::memcmp(blob, expect, sizeof expect) == 0, "blob bytes match version 1 layout");
    }

    // Corrupt blobs must be rejected and must not touch the output.
    {
        CitrineSettings base;
        base.ev = 7.0f;
        uint8_t good[kCitrineBlobSize];
        EncodeCitrineSettings(CitrineSettings{}, good);

        auto expectCorrupt = [&](const uint8_t* data, size_t n, const char* msg) {
            CitrineSettings out = base;
            BlobStatus st = DecodeCitrineSettings(data, n, out);
            CHECK(st == BlobStatus::Corrupt && out.ev == 7.0f, msg);
        };

        uint8_t badMagic[kCitrineBlobSize];
        std::memcpy(badMagic, good, sizeof good);
        badMagic[0] = 'X';
        expectCorrupt(badMagic, sizeof badMagic, "wrong magic is Corrupt, output unchanged");
        expectCorrupt(good, 5, "truncated blob is Corrupt, output unchanged");
        expectCorrupt(good, 0, "empty blob is Corrupt, output unchanged");

        uint8_t longBlob[kCitrineBlobSize + 1];
        std::memcpy(longBlob, good, sizeof good);
        longBlob[9] = 0;
        expectCorrupt(longBlob, sizeof longBlob, "version 1 with extra bytes is Corrupt");

        uint8_t zeroVersion[kCitrineBlobSize];
        std::memcpy(zeroVersion, good, sizeof good);
        zeroVersion[4] = 0;
        expectCorrupt(zeroVersion, sizeof zeroVersion, "version 0 is Corrupt");

        const float bad[] = {NAN, INFINITY, -INFINITY, kCitrineEVMax + 0.5f, kCitrineEVMin - 0.5f};
        for (float ev : bad)
        {
            CitrineSettings tmp;
            tmp.ev = ev;
            uint8_t blob[kCitrineBlobSize];
            EncodeCitrineSettings(tmp, blob);
            expectCorrupt(blob, sizeof blob, "non-finite or out-of-range EV is Corrupt");
        }
    }

    // A blob from a later build is reported as such, not parsed.
    {
        uint8_t future[kCitrineBlobSize];
        EncodeCitrineSettings(CitrineSettings{}, future);
        future[4] = kCitrineBlobVersion + 1;
        CitrineSettings out;
        out.ev = 7.0f;
        CHECK(DecodeCitrineSettings(future, sizeof future, out) == BlobStatus::NewerVersion && out.ev == 7.0f,
              "newer version is NewerVersion, output unchanged");
    }

    if (failures)
    {
        std::printf("%d settings check(s) failed\n", failures);
        return 1;
    }
    std::printf("all settings checks passed\n");
    return 0;
}
