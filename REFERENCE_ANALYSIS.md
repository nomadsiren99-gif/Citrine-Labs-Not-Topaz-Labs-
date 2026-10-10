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
