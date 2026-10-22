#include "CitrineSettings.h"

#include <cmath>
#include <cstring>

namespace
{
constexpr uint8_t kMagic[4] = {'C', 'T', 'R', 'N'};
constexpr size_t kEVOffset = 5;
}

void EncodeCitrineSettings(const CitrineSettings& s, uint8_t* out)
{
    std::memcpy(out, kMagic, sizeof kMagic);
    out[4] = kCitrineBlobVersion;
    uint32_t bits;
    std::memcpy(&bits, &s.ev, sizeof bits);
    for (size_t i = 0; i < 4; i++)
        out[kEVOffset + i] = (uint8_t)(bits >> (8 * i));
}

BlobStatus DecodeCitrineSettings(const uint8_t* data, size_t size, CitrineSettings& out)
{
    if (size < kEVOffset || std::memcmp(data, kMagic, sizeof kMagic) != 0)
        return BlobStatus::Corrupt;
    if (data[4] > kCitrineBlobVersion)
        return BlobStatus::NewerVersion;
    if (data[4] == 0 || size != kCitrineBlobSize)
        return BlobStatus::Corrupt;

    uint32_t bits = 0;
    for (size_t i = 0; i < 4; i++)
        bits |= (uint32_t)data[kEVOffset + i] << (8 * i);
    float ev;
    std::memcpy(&ev, &bits, sizeof ev);
    if (!std::isfinite(ev) || ev < kCitrineEVMin || ev > kCitrineEVMax)
        return BlobStatus::Corrupt;

    out.ev = ev;
    return BlobStatus::Ok;
}
