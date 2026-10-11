# Bundled models

## treenet_denoise_super_light.onnx

- Model: TreeNet Denoise Super Light, from the RawLab catalog (RawForge project).
- Source: https://huggingface.co/AyedaOk/rawlab-models (path `models/rawdenoise-rawforge-shadowweightedl1-super-light/1.0/model.onnx`)
- Size: 835,451 bytes
- SHA-256: 1684f549fec52812ffacc3020bfd61c5fde610e77460fcc03b2a7003f61e60dd (matches the manifest)
- Licence: MIT, per the RawForge repository (`LICENSE-RawForge-MIT.txt`, https://github.com/rymuelle/RawForge). The catalog marks the model licence as "publisher-declared"; not independently verified beyond that.
- Interface: inputs `input` float16 [batch, 3, H, W] (linear Rec.2020 RGB, 0..1) and `cond` float16 [batch, 1] = min(ISO, 65535) / 6400. Output `output` float16 [batch, 3, H, W]. Tiles: 256 px with 64 px overlap.
- Runtime check: ONNX Runtime 1.31 on a 64x64 synthetic tile, MSE vs clean reduced 4.1x. Not yet verified on photographs.
- Not loadable by `tract-onnx` 0.21 (dynamic `Pad` input is rejected).
