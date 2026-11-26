# Ren’Py source and relinking

Compilation and local packaging do not establish redistribution readiness. Before manually distributing any Ren’Py target, verify its matching source, notices, and tested replacement or relink path. The default Android and Windows Managers do not embed Ren’Py.

## What to distribute alongside each core ZIP

- **Exact product and fork source.** Retain the product commit, `Cargo.lock`, the `ThirdParty/cpal` gitlink and `licenses/pinned-sources.txt` from staging. For product [9cc0e8eb](https://github.com/Project-Shinkirou/ShinkiEMU/tree/9cc0e8eb7993b0cfa54ce3e46664c05b951805a6), the lock selects [adapter 0072512d](https://github.com/Project-Shinkirou/renpy/tree/0072512dc0181fa737784f309fbf228b605617dd), including the host bridge, display override and Android build recipe. Use the product's workspace patches when resolving the adapter's ABI and build dependencies. Verify anonymous retrieval of every exact tree used; the core's upstream identity is not its adapter pin.
- **Exact native runtime source.** The SDK is `8.5.3.26051504`, ZIP SHA-256 `ff57648f9c04f27e381c48af6d8e3ee3cdec296bed4d3831f47f09b0a71b505e`. Start with [Ren’Py 39895c1e](https://github.com/renpy/renpy/tree/39895c1e017f0b36ffea2447d97eccd69d76ee1c) and [renpy-build 7bfab40c](https://github.com/renpy/renpy-build/tree/7bfab40c1174f622f644b24669afd5fb167fbb79): retain the source archives, embedded Pygame_SDL2 source, patches, build tasks and generated configuration. Collect and verify the extra Git inputs in the locked adapter's `licenses/renpy/native-sources.json` (including Windows CPython-MinGW and macOS PyObjus). Upstream tasks still fetch some tags and branches; enforce the manifest's exact trees before compiling and keep the intake procedure. SDK binaries are the baseline, not corresponding source.
- **Python and notices.** Retain the adapter's `licenses/renpy/python-sources.json`, its hashed source distributions and build/install recipe. The desktop SDK ships bytecode, including chardet 5.2.0 (LGPL) and certifi 2026.4.22 (MPL-2.0); Android uses the inventoried pure Python artifacts and excludes host-native wheels. Keep `LICENSE-RenPy.txt`, the exact runtime CPython notice, `python/chardet/LICENSE`, `python/certifi/LICENSE`, the notices for each shipped native and Python component, and the font licenses beside the fonts. Check the actual package against the [upstream inventory](https://www.renpy.org/doc/html/license.html) and the selected link inputs.
- **Relink inputs and instructions.** Include the editable source of the shipped LGPL components and the build procedure or relinkable objects needed to replace them in the combination. Preserve archive order, linker response files or expanded invocations, compiler/SDK identities and options; check generated headers, `config.log` and link maps for the actual inputs. Verify the FFmpeg LGPL configuration and the FreeType license option actually used. Account for extra runtime libraries from `build/dependencies.toml`, vcpkg baseline/overlays or platform packaging. ShinkiEMU permits modification of LGPL components, compatible replacement and reverse engineering to debug those modifications.
- **A receipt tying material to the binary.** Record ZIP and payload hashes, source commit/archive digests, the test patch and validation log, and where recipients can retrieve the source and relink material. Prefer one common corresponding-source archive plus a per-target link-input archive where needed. Make the material available alongside the binary and test retrieval and replacement from a clean environment. The packager does not generate this bundle.

Verify these inputs from `renpy-build/source/`:

| File | SHA-256 |
| --- | --- |
| `ffmpeg-4.3.1.tar.gz` | `45035f15d6f192772de2309c846e1d60472694f479679354a39c699719e53772` |
| `fribidi-1.0.7.tar.bz2` | `5ab5f21e9f2fc57b4b40f8ea8f14dba78a5cc46d9cf94bc5e00a58e6886a935d` |
| `Python-3.12.8.tar.xz` | `c909157bb25ec114e5869124cc2a9c4a4d4c1e957ca4ff553f1edc692101154e` |
| `freetype-2.13.3.tar.gz` | `5c3a8e78f7b24c20b25b54ee575d6daa40007a5f4eea2845861c3409b3021747` |

## Reproduce the native replacement

1. Stage the original with `cargo xtask package --target <triple> --core renpy --version <version> --out <new-dir>` on the supported runner. Keep the original ZIP and receipts untouched. Isolate the workspace and build outputs for each ABI and variant.
2. Provision the [upstream prerequisites](https://github.com/renpy/renpy-build/blob/7bfab40c1174f622f644b24669afd5fb167fbb79/README.rst) on an isolated Ubuntu 24.04 x64 builder. Check out both pinned upstream trees, with Ren’Py in `renpy-build/renpy/`, and verify the nested revision after any upstream setup. Keep `tasks/{ffmpeg,fribidi,renpython}.py`, their expanded build/link commands and the inputs below. Apply an ABI-preserving source change to FriBidi or FFmpeg with an observable marker and keep the diff. Rebuild from clean affected outputs.
3. On desktop, replace the combined library in a disposable staged package, preserving the CPython ABI and the imports of the core and its display override. On Android, relink the whole adapter. Check machine type/slices, loader paths, ABI query, dependency resolution and mechanical launch, and observe the marker from the rebuilt library. Recompute `core.json` payload hashes and verify the separate test package, or use a standalone loader harness. The test must not bypass Manager's package/catalog verification.

| Standalone target | Recipe and package path | Link and redistribution evidence |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | `./build.sh --platform windows --arch x86_64 --python 3`; replace root `librenpython.dll`, retaining compatible `libpython3.12.dll` (target CPython-MinGW 3.12.7), `libwinpthread-1.dll` and `Resources/overrides/display.pyd`. The adapter uses SDK-derived MSVC import libraries. | Keep the upstream ordered link inputs and CPython import library, with `llvm-mingw-20241217-ucrt-ubuntu-20.04-x86_64.tar.xz`; record the product MSVC/SDK separately. Obtain the matching winpthreads source/license and the ANGLE binary notices and provenance: the upstream build tree carries the exact root `libEGL.dll`/`libGLESv2.dll` prebuilts, not their source/build provenance. Account for any added CRT or graphics redistributable. Compare PE imports/exports and verify the loader uses the staged DLLs. |
| `x86_64-unknown-linux-gnu` | `./build.sh --platform linux --arch x86_64 --python 3`; replace `Resources/librenpython.so` (includes target CPython 3.12.8). Verify `Resources/display.so` and the `$ORIGIN/Resources` (core) / `$ORIGIN` (display) search paths. | Keep the Ubuntu Focal amd64 sysroot package identities from `tasks/sysroot.py`, LLVM/LLD 18 and the ordered static inputs to the DSO. Check ELF version needs and actual SDL system dependencies, including libusb if selected. Test without a system Ren’Py installation. |
| `aarch64-apple-darwin` | `./build.sh --platform mac --arch x86_64,arm64 --python 3` builds universal `librenpython.dylib` (target CPython 3.12.8). Stage it in `Resources/` with ID `@rpath/librenpython.dylib`. The universal core and display look in `@loader_path/Resources` and `@loader_path`, respectively. | Acquire `tars/MacOSX12.3.sdk.tar.bz2` separately; record its identity, LLVM tools, link plans and upstream deployment flags (arm64 11.0 and x86_64 10.10). Inspect both Mach-O slices after install-name changes, use independent local signing if needed, and test the arm64 package in an arm64 process. |
| `x86_64-apple-darwin` | Same universal runtime recipe; stage and verify this Rust-target package separately. The stock baseline is `lib/py3-mac-universal/librenpython.dylib`, not the separate `renpy.app` copy. | Keep the x86_64 slice’s actual linker inputs and loader result. Test in an x86_64 process on a supported host; an arm64 load does not validate this row. |
| `aarch64-linux-android` (`arm64-v8a`) | Run the locked runtime recipe with `--target aarch64-linux-android`; package the rebuilt `shinki_emu_renpy.so` with this ABI's `RENPY_STATIC_ROOT`. | Keep the AArch64 archives and symbol map, generated headers, core/display objects, resources and final link command. Check 16 KiB LOAD alignment, `shinki_emu_query`, actual system/shared-C++ dependencies and the marker on this ABI. |
| `x86_64-linux-android` (`x86_64`) | Run the locked runtime recipe with `--target x86_64-linux-android`; repeat the link and package with this ABI's `RENPY_STATIC_ROOT`. | Keep the x86_64 closure and test as an x86_64 process. Neither an ARM64 fixture nor a historical embedded APK validates this row. |

Inspect the staged ZIP before replacement. The Linux dependency collector can add a root copy of `librenpython.so` alongside `Resources/librenpython.so`; replace every packaged copy and verify which path the loader resolves. On Linux/macOS, the desktop adapter's `LICENSE-CPython.txt` is taken from the **host** Python 3.12.7 build input; also retain the target 3.12.8 source license with the runtime. Keep the desktop build environment free of an unintended `SHINKI_EMU_FFMPEG_INSTALLED`, which adds a second FFmpeg link input.

**Android runtime command**, from the provisioned Linux builder, once per ABI:

```sh
python3 "$adapter/shinki/android_runtime.py" \
  --build-root "$renpy_build" --target "$target" \
  --output "$out/runtime" --host-python "$host_python"
RENPY_STATIC_ROOT="$out/runtime" cargo xtask package \
  --target "$target" --core renpy --version "$version" --out "$out/package"
```

Use absolute paths and run packaging from the product root. The host interpreter is CPython **3.12.7** with Cython **3.2.4**; the archives use target CPython **3.12.8**. Place the SHA-256-checked `android-ndk-r29-linux.zip` and `CubismSdkForNative-4-r.6.2.zip` from the adapter's `shinki/sdk.json` in `renpy-build/tars/`. The upstream recipes build private archives at API 21; the product links them at **API 28** with **NDK r30 `30.0.16248370`**. The adapter links CPython, Ren’Py builtins, patched offscreen SDL2 and the codecs into the standalone DSO. Preserve `runtime.json`, `include/`, `lib/` (including `lib/pkgconfig/shinki-renpy-runtime.pc`), `symbols.map` and `Resources/`. Regenerate private symbol renaming using the locked recipe when substituting a library.

An archive built with r30 can test substitution and the final link, but it does not reproduce the locked r29 source build. A host-only function test or ELF inspection does not validate Android loading. Verify the rebuilt standalone DSO with Android's loader on the matching ABI and retain the result with that package.

**Restricted inputs.** Give acquisition instructions and include permitted notices for Apple's SDK and the Cubism header SDK. Do not distribute their SDK archives or game-provided Cubism binaries. Check header permissions and which inputs contribute code to the shipped outputs before distributing relink material. Exclude optional Steamworks inputs unless their provenance and redistribution rights are established.

**Embedded Android builds.** The LGPL objects are in `libshinki_emu_manager.so`. Supply that APK's matching source or relinkable objects, ordered link inputs and flags, assets and assembly instructions. Verify a rebuilt APK with an independent test signature; production signing assets are not relink inputs.

**Quick Python replacement check (Windows).** [`Tools/renpy-check-python-replacement.py`](../../../Tools/renpy-check-python-replacement.py) accepts the pinned SDK ZIP, the exact adapter checkout and `chardet-5.2.0.tar.gz` from `python-sources.json`. It verifies the archive and DLL hashes, imports stock chardet in the SDK interpreter, replaces it with locally modified corresponding source and verifies the module path, marker and detector in a fresh process. This checks Python replacement without a game; the six native link and package gates still apply.

```sh
python Tools/renpy-check-python-replacement.py "$sdk_zip" "$adapter" "$chardet_sdist"
```
