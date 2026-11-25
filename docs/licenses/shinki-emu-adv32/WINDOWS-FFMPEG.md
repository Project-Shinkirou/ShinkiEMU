# Shinki ADV32: Windows x64 FFmpeg source and replacement

The Windows standalone `shinki_emu_adv32.dll` uses shared FFmpeg 7.1.2 libraries. The reference Windows build reported LGPL-2.1-or-later. Keep the FFmpeg copyright and license text (`licenses/ffmpeg/copyright`) with the binary. Recipients may modify the FFmpeg libraries and reverse engineer the combined work to debug those modifications as permitted by the LGPL.

## Corresponding source and build inputs

- FFmpeg: [n7.1.2 source archive](https://codeload.github.com/FFmpeg/FFmpeg/tar.gz/refs/tags/n7.1.2). SHA-512: `8411c45f71d2d61184b11e2a786137044a80d9b979a7e2e8513efc5e716b3360bff4533a13875dd4bca492b97b97f0384f7fb4f3d796802e81981b0857d18a2b`.
- vcpkg: [0c2a9ace1964b818b15f7ae50ba1c42404918a7b](https://github.com/microsoft/vcpkg/tree/0c2a9ace1964b818b15f7ae50ba1c42404918a7b). The [pinned registry](https://raw.githubusercontent.com/microsoft/vcpkg/0c2a9ace1964b818b15f7ae50ba1c42404918a7b/versions/f-/ffmpeg.json) resolves `7.1.2#0` to Git tree `59518ca90a049b55ae15820a6ccfb5165af2e359`. That tree holds the portfile, its full patch list and the patch sources. The registry checkout's top-level `ports/ffmpeg` can describe a newer release; use the historical tree.
- Product configuration: `build/vcpkg/ffmpeg/vcpkg.json`, `build/dependencies.toml` and `Source/Tools/shinki-emu-build/src/vcpkg.rs` in the ShinkiEMU source named by the package's `source-commit.txt`. The manifest disables default features and explicitly selects `avcodec`, `avdevice`, `avfilter`, `avformat`, `swresample` and `swscale`; `avutil` is built with FFmpeg. It also selects host `pkgconf`. Windows uses the stock `x64-windows` triplet (shared libraries and shared MSVC CRT), with no FFmpeg port overlay. The reference configure enabled shared linking and disabled static linking and autodetection; GPL, version3 and nonfree features were not enabled.

A public asset must make the matching source, patches and build recipe available to recipients, with the configuration and toolchain recorded for that exact asset. A link to an unpatched upstream tarball or the Rust license report alone does not supply those inputs.

## Build replacement DLLs

Use an x64 Visual Studio Developer PowerShell with MSVC and the Windows SDK. Keep the checkout and build paths short and free of spaces. Set `$ProductSource` to the exact ShinkiEMU checkout or source archive named above. In a new working directory:

```powershell
git clone https://github.com/microsoft/vcpkg.git
$Vcpkg = (Resolve-Path ./vcpkg).Path
git -C $Vcpkg switch --detach 0c2a9ace1964b818b15f7ae50ba1c42404918a7b
& "$Vcpkg/bootstrap-vcpkg.bat" -disableMetrics
$Install = Join-Path $PWD 'ffmpeg-install'
$env:VCPKG_BINARY_SOURCES = 'clear'
& "$Vcpkg/vcpkg.exe" install --triplet x64-windows --host-triplet x64-windows `
  "--x-manifest-root=$ProductSource/build/vcpkg/ffmpeg" `
  "--x-install-root=$Install" --x-abi-tools-use-exact-versions
```

The release DLLs are under `$Install/x64-windows/bin`. For a modified build, use `git archive 59518ca90a049b55ae15820a6ccfb5165af2e359` in the pinned vcpkg repository to export the port tree into a separate overlay's `ffmpeg/` directory. Add a patch for your change to that directory and to the portfile's `PATCHES` list. Pass `--overlay-ports=<overlay directory>` using a fresh install root. Retain the x64 architecture, compatible exports and DLL names.

## Use replacement DLLs

After installation or extraction of the core, make a backup and fully quit Manager and any other host using the core. Replace these DLLs **beside `shinki_emu_adv32.dll`** as a set, using the release files from the rebuild:

- `avcodec-61.dll`, `avdevice-61.dll`, `avfilter-10.dll`, `avformat-61.dll`, `avutil-59.dll`, `swresample-5.dll`, `swscale-8.dll`.

Restart the host and check that it loaded these files from the core directory. An already-loaded DLL with the same basename can take precedence on Windows. Use a fresh process with just this core for the replacement check. Manager verifies the unmodified package on installation; replace files only in the extracted or installed local copy, not in the official ZIP. An update can restore the shipped files.
