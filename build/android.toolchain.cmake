# Cargo's Android package command supplies these from the pinned NDK and target.
set(ANDROID_ABI "$ENV{ANDROID_ABI}")
set(ANDROID_PLATFORM "android-$ENV{SHINKI_EMU_ANDROID_NATIVE_API}")
file(TO_CMAKE_PATH "$ENV{ANDROID_NDK_HOME}" shinki_android_ndk)
include("${shinki_android_ndk}/build/cmake/android.toolchain.cmake")
