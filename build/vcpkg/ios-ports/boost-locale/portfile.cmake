# vcpkg boost-locale 1.91.0 (tree d42677f09dbe9a6e233b0cef8646ae133cb24887).
# Backport the source-group fix from boostorg/locale f51b58cd34a2.
vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO boostorg/locale
    REF boost-${VERSION}
    SHA512 bcdd96e856facc8d585c696066757ae8138283bad6b1d14a1ab3083b53098d997ed42de6f4498aec47f365d6e2c3387376dd8436792d3809760287e131cd4e11
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
