file(TO_CMAKE_PATH "$ENV{OHOS_SDK_NATIVE}" SHINKI_OHOS_NATIVE)
include("${SHINKI_OHOS_NATIVE}/build/cmake/ohos.toolchain.cmake")
# The SDK clang locates its own toolchain. --gcc-toolchain is unused and makes
# Meson's compiler probes fail under -Werror=unused-command-line-argument.
unset(CMAKE_C_COMPILER_EXTERNAL_TOOLCHAIN)
unset(CMAKE_CXX_COMPILER_EXTERNAL_TOOLCHAIN)
unset(CMAKE_ASM_COMPILER_EXTERNAL_TOOLCHAIN)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
