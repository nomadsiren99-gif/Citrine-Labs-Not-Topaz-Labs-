// Host simulation for RustExposure.cpp. Drives PluginMain(filterSelectorStart) with a hand-built
// FilterRecord and checks tiling, channel layout, row padding and the depth guard.
// This does NOT prove anything about Photoshop. It checks the shim's own logic only.
//
// Build and run: see plugins/photoshop-native/tests/run_host_sim.sh

#include "PIFilter.h"
#include "PIDefines.h"

#include <cstdio>
#include <cstring>
#include <vector>

extern "C" int32_t pe_apply_exposure_rgba8(uint8_t* pixels, uint32_t width, uint32_t height,
                                           size_t row_bytes, float ev);
DLLExport MACPASCAL void PluginMain(const int16 selector, FilterRecordPtr filterRecord,
                                               intptr_t* data, int16* result);

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

// Host state the fake advanceState() serves. Input and output are fixed buffers, so the tile
// rectangles only decide which region the plug-in reads and writes.
struct Host {
    std::vector<uint8_t> in;     // channels-interleaved source, row stride inRowBytes
    std::vector<uint8_t> out;    // destination, row stride outRowBytes
    int32 inRowBytes;
    int32 outRowBytes;
    int32 channels;
    int32 width;
    int32 height;
    int advanceCalls;
};

static Host* gHost = nullptr;
static FilterRecord* gRec = nullptr;

// Like a real host: point inData/outData at the top-left pixel of the current inRect/outRect.
static OSErr FakeAdvanceState(void)
{
    gHost->advanceCalls++;
    const size_t inOff = (size_t)gRec->inRect.top * gHost->inRowBytes + (size_t)gRec->inRect.left * gHost->channels;
    const size_t outOff = (size_t)gRec->outRect.top * gHost->outRowBytes + (size_t)gRec->outRect.left * gHost->channels;
    gRec->inData = gHost->in.data() + inOff;
    gRec->outData = gHost->out.data() + outOff;
    return noErr;
}

static void RunFilter(Host& host, FilterRecord& rec, int32 tileW, int32 tileH, int16* result)
{
    gHost = &host;
    gRec = &rec;
    rec.depth = 8;
    rec.planes = (int16)host.channels;
    rec.outTileWidth = (int16)tileW;
    rec.outTileHeight = (int16)tileH;
    rec.filterRect.top = 0;
    rec.filterRect.left = 0;
    rec.filterRect.bottom = (int16)host.height;
    rec.filterRect.right = (int16)host.width;
    rec.inRowBytes = host.inRowBytes;
    rec.outRowBytes = host.outRowBytes;
    rec.advanceState = FakeAdvanceState;
    intptr_t data = 0;
    PluginMain(filterSelectorStart, &rec, &data, result);
}

int main(void)
{
    // 4-channel case: 7x5 image, 3x2 tiles, 3 spare bytes after each output row.
    {
        Host h{};
        h.channels = 4;
        h.width = 7;
        h.height = 5;
        h.inRowBytes = h.width * 4;
        h.outRowBytes = h.width * 4 + 3;
        h.in.resize((size_t)h.inRowBytes * h.height);
        h.out.assign((size_t)h.outRowBytes * h.height, 0xEE);
        for (size_t i = 0; i < h.in.size(); i++)
            h.in[i] = (uint8_t)((i * 37 + 11) & 0xFF);

        // Full-frame reference through the same kernel.
        std::vector<uint8_t> ref = h.in;
        pe_apply_exposure_rgba8(ref.data(), 7, 5, 28, 1.0f);

        FilterRecord rec;
        std::memset(&rec, 0, sizeof rec);
        int16 result = 1;
        RunFilter(h, rec, 3, 2, &result);
        CHECK(result == noErr, "4ch: start returns noErr");
        CHECK(h.advanceCalls == 3 * 3, "4ch: 3x3 tiles requested from host");

        bool same = true, padOk = true;
        for (int32 y = 0; y < h.height; y++)
        {
            for (int32 x = 0; x < h.width; x++)
                for (int32 c = 0; c < 4; c++)
                    if (h.out[(size_t)y * h.outRowBytes + x * 4 + c] != ref[(size_t)y * 28 + x * 4 + c])
                        same = false;
            for (int32 p = 0; p < 3; p++)
                if (h.out[(size_t)y * h.outRowBytes + 28 + p] != 0xEE)
                    padOk = false;
        }
        CHECK(same, "4ch: tiled output equals full-frame output, alpha included");
        CHECK(padOk, "4ch: host row padding untouched");
    }

    // 3-channel case: alpha absent, so the plug-in must leave RGB equal to the reference.
    {
        Host h{};
        h.channels = 3;
        h.width = 5;
        h.height = 4;
        h.inRowBytes = h.width * 3;
        h.outRowBytes = h.width * 3;
        h.in.resize((size_t)h.inRowBytes * h.height);
        h.out.assign((size_t)h.outRowBytes * h.height, 0);
        for (size_t i = 0; i < h.in.size(); i++)
            h.in[i] = (uint8_t)((i * 53 + 7) & 0xFF);

        std::vector<uint8_t> rgba((size_t)5 * 4 * 4, 255);
        for (int32 y = 0; y < 4; y++)
            for (int32 x = 0; x < 5; x++)
                for (int32 c = 0; c < 3; c++)
                    rgba[(size_t)y * 20 + x * 4 + c] = h.in[(size_t)y * 15 + x * 3 + c];
        pe_apply_exposure_rgba8(rgba.data(), 5, 4, 20, 1.0f);

        FilterRecord rec;
        std::memset(&rec, 0, sizeof rec);
        int16 result = 1;
        RunFilter(h, rec, 2, 2, &result);
        CHECK(result == noErr, "3ch: start returns noErr");

        bool same = true;
        for (int32 y = 0; y < 4; y++)
            for (int32 x = 0; x < 5; x++)
                for (int32 c = 0; c < 3; c++)
                    if (h.out[(size_t)y * 15 + x * 3 + c] != rgba[(size_t)y * 20 + x * 4 + c])
                        same = false;
        CHECK(same, "3ch: tiled RGB output equals reference");
    }

    // Depth guard: 16-bit input must be rejected, not processed as 8-bit.
    {
        Host h{};
        h.channels = 4;
        h.width = 2;
        h.height = 2;
        h.inRowBytes = 8;
        h.outRowBytes = 8;
        h.in.assign(16, 1);
        h.out.assign(16, 1);
        FilterRecord rec;
        std::memset(&rec, 0, sizeof rec);
        int16 result = 0;
        gHost = &h;
        gRec = &rec;
        rec.depth = 16;
        rec.planes = 4;
        rec.outTileWidth = 2;
        rec.outTileHeight = 2;
        rec.filterRect.bottom = 2;
        rec.filterRect.right = 2;
        rec.advanceState = FakeAdvanceState;
        intptr_t data = 0;
        PluginMain(filterSelectorStart, &rec, &data, &result);
        CHECK(result == filterBadMode, "16-bit depth rejected with filterBadMode");
        CHECK(h.advanceCalls == 0, "16-bit depth: no tile requested");
    }

    if (failures)
    {
        std::printf("%d host-sim check(s) failed\n", failures);
        return 1;
    }
    std::printf("all host-sim checks passed\n");
    return 0;
}
