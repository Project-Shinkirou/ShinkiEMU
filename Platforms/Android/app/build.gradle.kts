plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Release signing material comes only from the environment. Blank values count as
// missing because GitHub Actions expands absent secrets to empty strings.
val releaseSigningVariables = listOf(
    "SHINKI_EMU_ANDROID_KEYSTORE",
    "SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD",
    "SHINKI_EMU_ANDROID_KEY_ALIAS",
    "SHINKI_EMU_ANDROID_KEY_PASSWORD",
)
val releaseSigning: Map<String, String> = releaseSigningVariables
    .mapNotNull { name ->
        providers.environmentVariable(name).orNull?.takeIf { it.isNotBlank() }?.let { name to it }
    }
    .toMap()
val missingReleaseSigning = releaseSigningVariables.filterNot(releaseSigning::containsKey)

val generatedRoot = layout.projectDirectory.dir(
    providers.environmentVariable("SHINKI_EMU_ANDROID_GENERATED_ROOT")
        .orElse("../generated").get()
)

android {
    namespace = "org.shinkirou.shinkiemu"
    compileSdk = 36
    buildToolsVersion = "36.0.0"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "org.shinkirou.shinkiemu"
        minSdk = 26
        targetSdk = 36
        versionCode = providers.environmentVariable("SHINKI_EMU_ANDROID_VERSION_CODE")
            .orElse("1")
            .get()
            .toInt()
        versionName = providers.environmentVariable("SHINKI_EMU_ANDROID_VERSION_NAME")
            .orElse("0.1.0")
            .get()
        ndk {
            abiFilters += providers.environmentVariable("SHINKI_EMU_ANDROID_ABI")
                .orElse("arm64-v8a,x86_64").get().split(",")
        }
    }

    signingConfigs.getByName("debug") {
        // Keep the generated test key beside the Gradle cache, not in ~/.android.
        storeFile = gradle.gradleUserHomeDir.resolve("shinkiemu-debug.keystore")
    }

    if (missingReleaseSigning.isEmpty()) {
        signingConfigs {
            create("release") {
                storeFile = file(releaseSigning.getValue("SHINKI_EMU_ANDROID_KEYSTORE"))
                storePassword = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEY_ALIAS")
                keyPassword = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEY_PASSWORD")
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
                enableV4Signing = true
            }
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
        }
        release {
            isMinifyEnabled = false
            isDebuggable = false
            if (missingReleaseSigning.isEmpty()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    sourceSets.getByName("main") {
        jniLibs.srcDir(generatedRoot.dir("jniLibs"))
        assets.srcDir(generatedRoot.dir("assets"))
    }

    packaging {
        jniLibs {
            // NativeActivity and dladdr need ordinary extracted filesystem paths.
            // RFVP is statically linked into the Manager library.
            useLegacyPackaging = true
        }
    }
}

// APKs are named ShinkiEMU-<versionName>-<buildType>.apk.
base {
    archivesName = "ShinkiEMU-${android.defaultConfig.versionName}"
}

// Without the release key the release variant would package an unsigned APK. Refuse
// to schedule any release APK/AAB packaging task instead, so no such file is ever produced.
val releasePackagingTasks = setOf("packageRelease", "packageReleaseBundle", "signReleaseBundle")
gradle.taskGraph.whenReady {
    if (missingReleaseSigning.isEmpty()) return@whenReady
    val releasePackaging = allTasks.filter { task ->
        task.project == project && task.name in releasePackagingTasks
    }
    if (releasePackaging.isNotEmpty()) {
        throw GradleException(
            "ShinkiEMU release packaging (${releasePackaging.joinToString { it.path }}) needs the " +
                "release signing key; missing or empty: ${missingReleaseSigning.joinToString()}. " +
                "Use :app:assembleDebug for a debug-signed APK."
        )
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    // Java/Kotlin side of native crates, staged by cargo xtask package.
    // (rustls-platform-verifier's org.rustls.platformverifier.CertificateVerifier,
    // called through JNI). Minify stays off; enabling it needs
    // `-keep class org.rustls.platformverifier.** { *; }`.
    implementation(fileTree(generatedRoot.dir("libs")) { include("*.aar") })
}
