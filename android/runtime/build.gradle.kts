plugins {
    id("com.android.application")
}

// Tyranor-Next 第三方 APK 运行时模块(轻量版)。
//
// 与 :app 的 tyranor 渠道不同:这里**只**包含
//   - TyranorRuntimeActivity(协议入口,无内容视图)
//   - MainActivity / NativeHost / DialogPresenter(游戏页与 JNI)
// 不编 Kotlin、Compose、Miuix、AppCompat,也不带游戏库、配置页、帮助页
// 及其资源。APK 体积由 so + libc++_shared 决定,壳本身接近零。
//
// 版本注入与 :app 保持一致:-PreleaseVersionName / -PreleaseVersionCode。

val injectedVersionName = (project.findProperty("releaseVersionName") as String?)
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val injectedVersionCode = (project.findProperty("releaseVersionCode") as String?)
    ?.trim()
    ?.toIntOrNull()

// 发布签名四件套与 :app 相同的环境变量;CI 上缺签名直接失败,不静默回落 debug。
val releaseKeystoreFile = System.getenv("YURIS_KEYSTORE_FILE")?.trim()?.takeIf { it.isNotEmpty() }
val releaseKeystorePassword = System.getenv("YURIS_KEYSTORE_PASSWORD")?.trim()
val releaseKeyAlias = System.getenv("YURIS_KEY_ALIAS")?.trim()?.takeIf { it.isNotEmpty() }
val releaseKeyPassword = System.getenv("YURIS_KEY_PASSWORD")?.trim()
val hasReleaseSigning = releaseKeystoreFile != null &&
    !releaseKeystorePassword.isNullOrEmpty() &&
    releaseKeyAlias != null &&
    !releaseKeyPassword.isNullOrEmpty()

if (!hasReleaseSigning && System.getenv("GITHUB_ACTIONS") == "true") {
    throw GradleException(
        "CI 构建缺少发布签名: 需要 YURIS_KEYSTORE_FILE / YURIS_KEYSTORE_PASSWORD / " +
            "YURIS_KEY_ALIAS / YURIS_KEY_PASSWORD 四个环境变量(由 secrets 注入)。",
    )
}

android {
    // 宿主只按 applicationId 前缀发现模块;namespace 跟 applicationId 一致,
    // Java 代码仍复用 org.yuris.kernel 的现成游戏页(清单里写全限定名)。
    namespace = "com.tyranor.runtime.yuris.kernel"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.tyranor.runtime.yuris.kernel"
        minSdk = 24
        targetSdk = 36
        versionCode = injectedVersionCode ?: 1
        versionName = injectedVersionName ?: "0.1.0"

        val abiFilter = (project.findProperty("abiFilter") as String?)?.trim()
        ndk {
            abiFilters += if (abiFilter.isNullOrEmpty()) {
                listOf("arm64-v8a", "x86_64")
            } else {
                listOf(abiFilter)
            }
        }
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = file(releaseKeystoreFile!!)
                storePassword = releaseKeystorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        debug {
            // 与正式模块并排安装时用 -PdebugAppIdSuffix=.debug 区分。
            val debugSuffix = (project.findProperty("debugAppIdSuffix") as String?)
                ?.trim()
                ?.takeIf { it.isNotEmpty() }
            if (debugSuffix != null) {
                applicationIdSuffix = debugSuffix
                versionNameSuffix = "-dev"
            }
        }
        release {
            isMinifyEnabled = false
            signingConfig = if (hasReleaseSigning) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            // 直接复用 :app 的 Java 游戏页/JNI 绑定,避免两份实现漂移。
            // Kotlin(游戏库/配置页/Compose)不在这里编译;GameLibrary 等纯
            // Java 工具类会被一起编译,但不会被引用,dex 增量可忽略。
            java.srcDir("../app/src/main/java")
            // so 由 tools/android/build.sh 交叉编译后落到 :app 的 jniLibs,
            // 这里只引用同一份产物,不再复制。
            jniLibs.srcDirs("../app/src/main/jniLibs")
        }
    }

    packaging {
        // 模块以分发体积优先:.so 在 APK 里压缩存放,安装时由系统解压。
        // (:app 保持不压缩,换取零解压启动;模块的 so 本身才是大头。)
        jniLibs.useLegacyPackaging = true
    }
}

if (hasReleaseSigning) {
    logger.lifecycle("运行时模块签名: release keystore ($releaseKeystoreFile, alias=$releaseKeyAlias)")
} else {
    logger.lifecycle("运行时模块签名: debug(本地调试用,不可用于分发)")
}
