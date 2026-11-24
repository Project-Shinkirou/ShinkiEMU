set(VCPKG_TARGET_ARCHITECTURE x64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)
# The pinned FFmpeg port only passes --target-os=linux on its Linux branch.
# Keep the DevEco compiler/sysroot for that port. Meson ports must see OHOS
# or the helper treats same-CPU Linux as native on Windows.
if(PORT STREQUAL "ffmpeg")
    set(VCPKG_CMAKE_SYSTEM_NAME Linux)
else()
    set(VCPKG_CMAKE_SYSTEM_NAME OHOS)
endif()
set(VCPKG_ENV_PASSTHROUGH OHOS_SDK_NATIVE)
set(VCPKG_CHAINLOAD_TOOLCHAIN_FILE "${CMAKE_CURRENT_LIST_DIR}/ohos-toolchain.cmake")
set(VCPKG_MAKE_BUILD_TRIPLET "--host=x86_64-unknown-linux-ohos")
set(VCPKG_CMAKE_CONFIGURE_OPTIONS -DOHOS_ARCH=x86_64 -DOHOS_PLATFORM_LEVEL=12 -DCMAKE_PLATFORM_NO_VERSIONED_SONAME=ON)
set(VCPKG_BUILD_TYPE release)
# Meson validates Windows prefixes using the OHOS target's POSIX path semantics.
if(CMAKE_HOST_WIN32 AND DEFINED CURRENT_PACKAGES_DIR)
    set(VCPKG_MESON_CONFIGURE_OPTIONS --prefix "//?/${CURRENT_PACKAGES_DIR}")
endif()
