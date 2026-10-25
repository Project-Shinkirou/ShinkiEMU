# Builtin arm64-osx plus ShinkiEMU's Apple deployment target and libass font provider.
# The deployment target lives here, not in the environment: an exported
# IPHONEOS_DEPLOYMENT_TARGET retargets host-triplet (macOS) tools to iOS.
set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)
set(VCPKG_CMAKE_SYSTEM_NAME Darwin)
set(VCPKG_OSX_ARCHITECTURES arm64)
set(VCPKG_OSX_DEPLOYMENT_TARGET 13.0)

# CoreText is the Apple system provider; never pick up a Homebrew fontconfig
# that the static pkg-config closure cannot resolve.
if(PORT STREQUAL "libass")
    set(VCPKG_MESON_CONFIGURE_OPTIONS -Dfontconfig=disabled -Dcoretext=enabled)
endif()
