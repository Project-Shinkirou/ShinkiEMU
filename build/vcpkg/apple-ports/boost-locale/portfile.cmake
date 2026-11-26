# vcpkg boost-locale 1.88.0#1 (tree 7021457116baad60cefbbf264e5757da594dabf4).
# Backport the source-group fix from boostorg/locale f51b58cd34a2.
vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO boostorg/locale
    REF boost-${VERSION}
    SHA512 4fb431048ccb17fadd62e67d65a3845e155ac060e49a08972eb9a7bbedd0f934bfcca0c566df29a720f14b66439276d8b41150e658a0651ed060ae50cf9f1ea3
    HEAD_REF master
    PATCHES cmake-source-group.patch
)

set(FEATURE_OPTIONS "")
include("${CMAKE_CURRENT_LIST_DIR}/features.cmake")
if(CMAKE_HOST_SYSTEM_NAME STREQUAL "Linux" AND VCPKG_TARGET_IS_MINGW)
    # mingw cross compile toolchain lacks std conv support
    list(APPEND FEATURE_OPTIONS "-DBOOST_LOCALE_ENABLE_STD=OFF")
endif()
boost_configure_and_install(
    SOURCE_PATH "${SOURCE_PATH}"
    OPTIONS ${FEATURE_OPTIONS}
)
