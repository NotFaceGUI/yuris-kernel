plugins {
    id("com.android.application")
    // 游戏库页用 Compose + Miuix(HyperOS 风);Java 配置页/游戏页保持原样。
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// 发布注入(全部可选,本地不传即用缺省值):
//   -PreleaseVersionName=0.1.0   APK versionName(CI 由 tag 去掉前缀得到)
//   -PreleaseVersionCode=123     APK versionCode(CI 用 run_number 单调递增)
// 版本只有这一个来源;不在仓库里另存一份,避免与 Cargo.toml 漂移。
val injectedVersionName = (project.findProperty("releaseVersionName") as String?)
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
val injectedVersionCode = (project.findProperty("releaseVersionCode") as String?)
    ?.trim()
    ?.toIntOrNull()

// 发布渠道:
//   standard(缺省) = 普通 APK,applicationId = org.yuris.kernel
//   tyranor        = Tyranor-Next 第三方 APK 运行时模块。宿主要求包名落在
//                    com.tyranor.runtime.* 命名空间(前缀是发现条件,不是
//                    安全边界),因此只能改 applicationId,不能只改清单。
// 用法: gradle -PyurisChannel=tyranor assembleRelease
val releaseChannel = (project.findProperty("yurisChannel") as String?)
    ?.trim()
    ?.lowercase()
    ?.takeIf { it.isNotEmpty() }
    ?: "standard"
if (releaseChannel != "standard" && releaseChannel != "tyranor") {
    throw GradleException(
        "yurisChannel 只接受 standard / tyranor,收到: $releaseChannel",
    )
}
val isTyranorModule = releaseChannel == "tyranor"

// 发布签名:四个环境变量由 CI 从 secrets 注入(见 docs/release.md)。
// 缺任何一项就**不是**正式签名 —— 本地允许回落 debug 签名(自用安装);
// CI 上则直接失败,绝不产出一个"看起来发布、其实是 debug key"的 APK。
// 变量名刻意加上 release 前缀:在下面的 `create("release") {}` 里,局部变量
// 与 SigningConfig 的属性同名时会被接收者属性遮蔽,曾导致 keyAlias/keyPassword
// 被赋成 null,packageRelease 报 "missing required property"。
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
            "YURIS_KEY_ALIAS / YURIS_KEY_PASSWORD 四个环境变量(由 secrets 注入)。" +
            "拒绝用 debug key 产出 Release APK。",
    )
}

android {
    namespace = "org.yuris.kernel"
    // Miuix 0.9.2 的 AAR 元数据要求 compileSdk >= 37(Android 17 SDK);
    // targetSdk 仍保持 36,不引入 Android 17 的运行时行为变更。
    compileSdk = 37

    defaultConfig {
        // Tyranor-Next 只按 com.tyranor.runtime.* 前缀识别第三方运行时模块;
        // 两个包名可以并存安装,互不影响。
        applicationId = if (isTyranorModule) {
            "com.tyranor.runtime.yuris.kernel"
        } else {
            "org.yuris.kernel"
        }
        minSdk = 24
        targetSdk = 36
        versionCode = injectedVersionCode ?: 1
        versionName = (injectedVersionName ?: "0.3.3") +
            if (isTyranorModule) "-tyranor" else ""
        // 模块 APK 与普通 APK 同时装在一台设备上时,桌面名称要能区分。
        manifestPlaceholders["appLabel"] = if (isTyranorModule) {
            "YU-RIS Kernel (Tyranor)"
        } else {
            "YU-RIS Kernel"
        }
        // 只打 64 位(架构文档 §2.5.9 开放问题 8:armeabi-v7a 待定):
        // arm64-v8a = 真机目标;x86_64 = 本机模拟器验证用。
        // ABI 过滤:默认两个都打;构建模拟器验证包时用 -PabiFilter=x86_64
        // 只打一个,避免低空间模拟器装不下。
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
            // 真机上已有同包名但不同签名的安装时,用后缀并排装一份调试壳
            // (不覆盖用户数据):gradle -PdebugAppIdSuffix=.newui assembleDebug
            val debugSuffix = (project.findProperty("debugAppIdSuffix") as String?)
                ?.trim()
                ?.takeIf { it.isNotEmpty() }
            if (debugSuffix != null) {
                applicationIdSuffix = debugSuffix
                versionNameSuffix = "-dev"
                manifestPlaceholders["appLabel"] = "YU-RIS Kernel (dev)"
            }
        }
        release {
            isMinifyEnabled = false
            // 有 secrets ⇒ 正式签名;没有 ⇒ debug 签名(仅本机自用调试)。
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

    buildFeatures {
        // 只有 LibraryActivity(Kotlin/Compose)用到;Java 壳不依赖。
        compose = true
    }

    // so 由 cargo 构建后落到 jniLibs;不在这里跑 cargo(保持 Gradle 离线可用)。
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    packaging {
        // 内置的 so 已 strip 过;不重复压缩,启动更快。
        jniLibs.useLegacyPackaging = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// 配置期打印一次签名来源,构建日志里能直接看出这个 APK 是不是正式签的。
if (hasReleaseSigning) {
    logger.lifecycle("APK 签名: release keystore ($releaseKeystoreFile, alias=$releaseKeyAlias)")
} else {
    logger.lifecycle("APK 签名: debug(本地调试用,不可用于分发)")
}

// 只用最通用的官方库:AppCompat(主题/控件/对话框)+ core(动画与兼容工具)。
// 不引 Material Components(本机 Gradle 缓存里没有,离线构建会失败)。
dependencies {
    // 配置页/游戏页(Java + AppCompat)。游戏画面仍走 TextureView + JNI,
    // 不因 UI 重构而改动渲染路径。
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("androidx.core:core:1.18.0")

    // 游戏库页(Compose + Miuix)。Miuix 0.9.2 的 AAR 元数据要求
    // compileSdk 37;传递依赖里的 Compose 版本与本地缓存一致(见探针)。
    implementation("top.yukonga.miuix.kmp:miuix-ui:0.9.2")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.compose.foundation:foundation:1.11.2")

    // JetBrains Compose 的 lifecycle 传递依赖要求 2.9.4/2.9.6;本机缓存
    // 只有 2.10.0,统一抬到 2.10.0(与 Compose 1.11.2 配套)。
    constraints {
        // AppCompat 1.6.1 默认拉 fragment 1.3.6(本机未缓存);抬到 1.8.6,
        // 与 Compose 1.11 的 activity/savedstate 组合一起验证过。
        implementation("androidx.fragment:fragment:1.8.6")
        implementation("androidx.lifecycle:lifecycle-common:2.10.0")
        implementation("androidx.lifecycle:lifecycle-runtime:2.10.0")
        implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
        implementation("androidx.lifecycle:lifecycle-viewmodel:2.10.0")
        implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
        implementation("androidx.lifecycle:lifecycle-viewmodel-savedstate:2.10.0")
        implementation("androidx.lifecycle:lifecycle-livedata:2.10.0")
        implementation("androidx.lifecycle:lifecycle-livedata-core:2.10.0")
        implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    }
}
