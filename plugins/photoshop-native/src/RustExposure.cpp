// Minimal Photoshop filter for milestone M0. Applies global exposure through the Rust engine (engine-ffi).
// Photoshop-specific work only: tiling, channel layout, error codes. No image maths here.
// Not verified in Photoshop yet. See PHOTOSHOP_INTEGRATION.md.

#include "PIFilter.h"
#include "PIDefines.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <vector>

// From crates/ffi (engine-ffi). Returns 0 on success, negative on error.
extern "C" int32_t pe_apply_exposure_rgba8(uint8_t* pixels, uint32_t width, uint32_t height,
                                           size_t row_bytes, float ev);

// shortcut: fixed EV, no dialog and no descriptor persistence yet; add both before M3
static const float kFixedEV = 1.0f;

static OSErr ProcessFilter(FilterRecordPtr fr)
{
    // shortcut: 8-bit RGB(A) only; other depths and modes are rejected by PiPL EnableInfo as well
    if (fr->depth != 8)
        return filterBadMode;
    const int32 channels = fr->planes;
    if (channels != 3 && channels != 4)
        return filterBadMode;

    const int32 tileW = fr->outTileWidth;
    const int32 tileH = fr->outTileHeight;
    if (tileW <= 0 || tileH <= 0)
        return filterBadParameters;

    fr->inputRate = (int32)1 << 16;
    fr->maskRate = (int32)1 << 16;

    const Rect area = fr->filterRect;
    for (int32 top = area.top; top < area.bottom; top += tileH)
    {
        for (int32 left = area.left; left < area.right; left += tileW)
        {
            Rect tile;
            tile.top = (int16)top;
            tile.left = (int16)left;
            tile.bottom = (int16)std::min<int32>(top + tileH, area.bottom);
            tile.right = (int16)std::min<int32>(left + tileW, area.right);

            fr->inRect = tile;
            fr->outRect = tile;
            fr->inLoPlane = 0;
            fr->inHiPlane = (int16)(channels - 1);
            fr->outLoPlane = 0;
            fr->outHiPlane = (int16)(channels - 1);

            OSErr err = fr->advanceState();
            if (err != noErr)
                return err;

            const int32 w = tile.right - tile.left;
            const int32 h = tile.bottom - tile.top;
            const size_t packedRow = (size_t)w * 4;
            std::vector<uint8_t> rgba(packedRow * (size_t)h);

            // Gather into RGBA8. Missing alpha is 255 and is never written back.
            const uint8_t* in = static_cast<const uint8_t*>(fr->inData);
            for (int32 y = 0; y < h; y++)
            {
                const uint8_t* src = in + (size_t)y * fr->inRowBytes;
                uint8_t* dst = rgba.data() + (size_t)y * packedRow;
                for (int32 x = 0; x < w; x++)
                {
                    dst[0] = src[0];
                    dst[1] = src[1];
                    dst[2] = src[2];
                    dst[3] = channels == 4 ? src[3] : 255;
                    src += channels;
                    dst += 4;
                }
            }

            if (pe_apply_exposure_rgba8(rgba.data(), (uint32_t)w, (uint32_t)h, packedRow, kFixedEV) != 0)
                return filterBadParameters;

            uint8_t* out = static_cast<uint8_t*>(fr->outData);
            for (int32 y = 0; y < h; y++)
            {
                uint8_t* dst = out + (size_t)y * fr->outRowBytes;
                const uint8_t* src = rgba.data() + (size_t)y * packedRow;
                for (int32 x = 0; x < w; x++)
                {
                    dst[0] = src[0];
                    dst[1] = src[1];
                    dst[2] = src[2];
                    if (channels == 4)
                        dst[3] = src[3];
                    dst += channels;
                    src += 4;
                }
            }
        }
    }
    return noErr;
}

DLLExport MACPASCAL void PluginMain(const int16 selector, FilterRecordPtr filterRecord, intptr_t* data, int16* result)
{
    (void)data;
    *result = noErr;
    switch (selector)
    {
        case filterSelectorAbout:
        case filterSelectorParameters:
            break;
        case filterSelectorPrepare:
            filterRecord->bufferSpace = 0;
            break;
        case filterSelectorStart:
            *result = ProcessFilter(filterRecord);
            break;
        default:
            break;
    }
}
