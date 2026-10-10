# Reference analysis

## Reference package

Folder: `/Users/dibesu/Documents/Reverse Topaz/Topaz Adjust AI 1.0.5 [FileCR]/Topaz Adjust AI 1.0.5 [FileCR]/`

| File | Size | Notes |
|---|---|---|
| `Topaz Adjust AI.exe` | 218,890,488 bytes | PE32, GUI, Intel 80386, for MS Windows. SHA-256 `0825696aa4cb3937313bfee5201772970a03d75adcf1de414f1356cabc7f4bf9` |
| `Instructions.txt` | 304 bytes | Read. See below. |
| `Reg.reg` | 242 bytes | **Not read in full, not merged.** |
| `Visit FileCR.url` | 119 bytes | **Not opened.** |

## Identity

- Product name: taken from the file name only. "Topaz Adjust AI".
- Version: folder name says 1.0.5. **Unverified.** The version resource could not be read with `strings`. The About dialog has not been checked, and the program has not been run.
- Product line: Adjust AI is a newer, now discontinued Topaz product. It is not Adjust 5. The spec's rule applies: do not treat it as the Adjust 5 reference.

## What was not done, and why

- The `.exe` was **not executed**. It is an unknown Windows binary and the spec requires an isolated Windows VM for any run.
- No slider sweeps or reference exports were made.
- `Instructions.txt` describes installing the program, starting a trial, merging the `.reg` file, blocking the network with a firewall, and disabling update checks, so that the "trial limitation will be gone". **This is a licence bypass. It was not followed and will not be.**
- The package appears to be a cracked distribution. Comparison work should use a copy that was lawfully obtained and licensed, and the output must be labelled as coming from that copy.

## Mapping to Adjust 5

None established. Per the spec, a mapping is recorded only with confidence `verified`, `approximate` or `not-comparable`. Current status: no mappings recorded.

## Reference target: Topaz Adjust 5 v5.1.0 (no binary available)

- The user does not have a lawful copy of Topaz Adjust 5 v5.1.0. No binary has been run, inspected or compared.
- Evidence available: the user's screenshots (see the memory note `topaz-adjust-screenshots`), and the spec.
- Every behaviour below is **UNVERIFIED** against Adjust 5.1.0 pixels. The screenshots show one preset state each, so they are not defaults or ranges.

| Control (panel label) | Screenshot observation (preset state) | Our engine | Confidence |
|---|---|---|---|
| Adaptive Exposure | 0.57 (Detail - Medium); 0.07 (Mild Contrast Pop) | `AdaptiveExposure.strength`, 0..1 | Label match only, unverified |
| Regions | 22 and 5 | `regions`, clamped 1..64 | Label match only; the ranges differ and are unverified |
| Contrast | -0.77 and 0.56 | `contrast`, mapped to 1 + c, clamped -1..1 | Label match only, mapping unverified |
| Brightness | 0.03 and 0.00 | `brightness_ev`, EV offset | Label match only, unit unverified |
| Protect Highlights / Shadows | 0.02 to 0.03 | `protect_*`, 0..1 | Label match only |
| Details Strength | 1.07 and 1.00 | `Details.strength`, 0..2 | Label match only |
| Detail Boost | 1.30 and 1.00 | `Details.boost`, 0..2 | Label match only |
| Threshold | 0.08 and 0.15 | `Details.threshold`, 0..1 (higher admits more) | Label match only; the direction is from the manual, unverified here |
| Radius | 30.00 and 25.00 | `Details.radius`, pixels, clamped 1..16 | Unit and scale unverified; the screenshot value is likely not in pixels |
| Sharpen | 1.00 | `Details.sharpen`, 0..2 | Label match only |
| Process Details Independently | unchecked | `process_independently` | Label match only |
| Color: Saturation, Boost, Hue, Color Regions, Adaptive Saturation | 1.00, 1.00, 0.00, 1, 0.00 | Not implemented | Not started |
| Local: Dodge/Burn/Brush Out/Smooth, Overall Strength, Brush Size, Opacity | 0.20, 100, 0.25 | Not implemented | Not started |

Note: the Details Radius value of 25 to 30 in the screenshots suggests a unit larger than our pixel radius. Do not match it without a test.

Fidelity claim: **none**. No pixel parity with Adjust 5.1.0 is claimed or measured.

## Static strings check of Topaz Adjust AI.exe (2026-10-10)

- Scope: read-only string extraction of the file on disk (SHA-256 `0825696a…c7f4bf9`, verified before reading). The binary was not run, not decompiled, and not analysed for licence or activation code.
- Identity from strings: product "Topaz Adjust AI", installer "TopazAdjustAI", "TopazAdjustBETA". Not Adjust 5. The version in the strings is not clear.
- Framework: Qt/QML with Boost. Of 492 QML file names, most are standard Qt Quick Controls boilerplate.
- Control labels such as "Adaptive Exposure", "Detail Boost" and "Dodge" do not appear as plain text. They are probably compressed or in resources. Nothing usable was extracted.
- Conclusion: no usable feature inventory without decompiling a cracked build, which is out of scope. No features were taken from it. The exe is not a reference for Adjust 5.
