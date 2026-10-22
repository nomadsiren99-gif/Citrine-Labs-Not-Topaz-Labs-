// Citrine filter settings and the versioned blob Photoshop stores in the smart filter.
// No Adobe SDK types here, so the codec builds and tests without the SDK headers.
#pragma once

#include <cstddef>
#include <cstdint>

struct CitrineSettings
{
    float ev = 1.0f; // exposure in stops; 1.0 matches the M0 fixed value
};

// Blob layout, version 1, 9 bytes, little-endian:
//   [0..3]  magic "CTRN"
//   [4]     format version
//   [5..8]  ev as IEEE-754 float32 bits
constexpr size_t kCitrineBlobSize = 9;
constexpr uint8_t kCitrineBlobVersion = 1;
constexpr float kCitrineEVMin = -10.0f;
constexpr float kCitrineEVMax = 10.0f;

enum class BlobStatus
{
    Ok,
    Corrupt,
    NewerVersion, // written by a later build; do not guess at its fields
};

// Writes exactly kCitrineBlobSize bytes to out.
void EncodeCitrineSettings(const CitrineSettings& s, uint8_t* out);

// Fills out only when the result is Ok, so a failed read leaves the caller's settings unchanged.
BlobStatus DecodeCitrineSettings(const uint8_t* data, size_t size, CitrineSettings& out);
