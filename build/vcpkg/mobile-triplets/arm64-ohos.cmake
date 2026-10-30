set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)
set(VCPKG_CMAKE_SYSTEM_NAME Linux)
# vcpkg scrubs the build environment; the chainloaded toolchain reads the SDK
# path from it.
set(VCPKG_ENV_PASSTHROUGH OHOS_SDK_NATIVE)
set(VCPKG_CHAINLOAD_TOOLCHAIN_FILE "${CMAKE_CURRENT_LIST_DIR}/ohos-toolchain.cmake")
set(VCPKG_MAKE_BUILD_TRIPLET "--host=aarch64-unknown-linux-ohos")
set(VCPKG_CMAKE_CONFIGURE_OPTIONS -DOHOS_ARCH=arm64-v8a -DOHOS_PLATFORM_LEVEL=12 -DCMAKE_PLATFORM_NO_VERSIONED_SONAME=ON)
if(PORT STREQUAL "libass")
    set(VCPKG_MESON_CONFIGURE_OPTIONS -Dfontconfig=disabled -Dcoretext=disabled -Ddirectwrite=disabled)
endif()
