# Original Anime4K v4.0.1

This backend uses the MIT-licensed [bloc97 Anime4K v4.0.1 Stable release](https://github.com/bloc97/Anime4K/releases/tag/v4.0.1). The [tag reference](https://api.github.com/repos/bloc97/Anime4K/git/ref/tags/v4.0.1) resolves to `4029bf701ecaa15f163cdc49cffe5501c1acf410`. The two vendored GLSL files are byte-identical to the selected [release ZIP](https://github.com/bloc97/Anime4K/releases/download/v4.0.1/Anime4K_v4.0.zip) entries and their corresponding files at that commit (`glsl/Restore/` and `glsl/Upscale/`). Copyright notices appear in the source files and [LICENSE](LICENSE).

| File | Observed SHA-256 |
| --- | --- |
| `Anime4K_v4.0.zip` (GitHub release asset 44982728) | `139cd282086457c5adc79caf7b75b8b825091d71c9b54958c18745fea62d7ed7` |
| `Anime4K_Restore_CNN_S.glsl` | `97c24dc370ab300c108bfaa09db7f175aeff343674842c299cf3940a3d330427` |
| `Anime4K_Upscale_CNN_x2_S.glsl` | `4c53ec2e287908f7ee7bcb266b0170421626d663576468b7d7dafc62962649a4` |
| `LICENSE` (tagged repository) | `5bad448b737378e3d0c977ad0d0521fa37ad279a7e76ea9a31d9257eeb6953f5` |

## Filter contract

| Manager kind | Graph | Extent |
| --- | --- | --- |
| `RestoreSmall` | Four Restore convolutions, final RGB residual add | Input extent |
| `UpscaleSmall2x` | Four Upscale convolutions, depth-to-space and residual add | Exactly 2× per axis |
| `RestoreSmallThenUpscaleSmall2x` | Restore's unclamped float result → Upscale | Exactly 2× per axis |

Both CNNs consume RGB in the incoming `Rgba8Unorm` value space. The first layer has zero alpha weights; there is no explicit luma conversion. Restore predicts distinct RGB residuals. Upscale predicts one scalar residual per output pixel, shared by RGB. The four residual channels map to offsets `(0,0), (1,0), (0,1), (1,1)` in a 2×2 block. The base image is sampled bilinearly at destination pixel centers, with clamp-to-edge addressing. Manager preserves same-size alpha or bilinearly resamples it at 2×; the upstream mpv shader's video-only residual is not added to alpha. Selecting an Upscale preset requests exact 2×, so the mpv viewport-size `WHEN` predicate does not apply.

Convolutions evaluate GLSL's column-major kernels and positive-then-negative ReLU terms in their original order in f32. Feature boundaries and the private Restore→Upscale boundary use signed, unclamped `Rgba16Float`; only the public stage output is clamped and quantized to `Rgba8Unorm`. The caller owns the final texture and submits the encoder. The backend reuses two feature textures and caches their bindings; the composed variant uses one additional float image. No external compiler is loaded for these built-ins. The Magpie/format-4 path is independent.

## Reproduce

From this directory, with Python 3.10+ and byte-preserving shell redirection:

```sh
python convert.py --check --release /path/to/Anime4K_v4.0.zip
python convert.py > cnn.wgsl
```

The first command verifies the ZIP and member checksums and the checked-in conversion. `--check` also works without the ZIP. Manager embeds `runtime.wgsl` and generated `cnn.wgsl`; its normal build needs no Python, network access or GLSL compiler for this backend.

`parity.txt` contains five synthetic RGBA8 fixtures rendered by `reference.py` on OpenGL 3.3 (NVIDIA GeForce RTX 4070 Laptop GPU, driver 621.01). For each fixture the file contains a name and extent, input bytes, and the Restore, Upscale, and composed output bytes, in row-major hexadecimal. The reference compiles the original GLSL hook bodies directly with an offline mpv-style texture wrapper, FP16 feature targets, and the documented alpha contract. Regenerate with `python reference.py > parity.txt` using ModernGL and an offscreen OpenGL adapter. Small driver-dependent quantization differences are expected.

Run the opt-in native GPU pixel test from the repo root:

```sh
cargo test -p shinki-emu-manager --lib anime4k_original_matches_upstream_glsl -- --ignored --nocapture
```

The test compares every byte with a one-code-point tolerance and checks exact Restore alpha, extent rejection, odd/tail dispatch, thin and one-pixel inputs, and a second frame using the same textures.
