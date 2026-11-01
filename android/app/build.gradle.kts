plugins {
    id("com.android.application")
}

android {
    namespace = "org.yuris.kernel"
    compileSdk = 36

    defaultConfig {
        applicationId = "org.yuris.kernel"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
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

    buildTypes {
        release {
            isMinifyEnabled = false
            // 未配 keystore 时用 debug 签名,保证可直接安装调试(自用分发)。
            // 正式发布时在 signingConfigs 里换自己的 keystore。
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
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

// 只用最通用的官方库:AppCompat(主题/控件/对话框)+ core(动画与兼容工具)。
// 不引 Material Components(本机 Gradle 缓存里没有,离线构建会失败)。
dependencies {
    // 本机 Gradle 缓存里**依赖树完整**的一套(离线/受限网络唯一可用组合):
    // appcompat 1.6.1 + core 1.13.1 + activity 1.8.1 + fragment 1.7.1 +
    // lifecycle 2.7.0 + annotation-jvm 1.8.x。更高版本会拉未缓存的传递依赖。
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("androidx.core:core:1.13.1")
    constraints {
        implementation("androidx.appcompat:appcompat:1.6.1")
        implementation("androidx.appcompat:appcompat-resources:1.6.1")
        implementation("androidx.core:core:1.13.1")
        implementation("androidx.activity:activity:1.8.1")
        implementation("androidx.fragment:fragment:1.7.1")
        implementation("androidx.lifecycle:lifecycle-common:2.7.0")
        implementation("androidx.lifecycle:lifecycle-runtime:2.7.0")
        implementation("androidx.lifecycle:lifecycle-viewmodel:2.7.0")
        implementation("androidx.lifecycle:lifecycle-viewmodel-savedstate:2.7.0")
        implementation("androidx.lifecycle:lifecycle-livedata:2.7.0")
        implementation("androidx.lifecycle:lifecycle-livedata-core:2.7.0")
        implementation("androidx.lifecycle:lifecycle-process:2.7.0")
        implementation("androidx.savedstate:savedstate:1.2.1")
        implementation("androidx.arch.core:core-common:2.2.0")
        implementation("androidx.arch.core:core-runtime:2.2.0")
        implementation("androidx.collection:collection:1.2.0")
        implementation("androidx.annotation:annotation:1.8.2")
        implementation("androidx.annotation:annotation-jvm:1.8.2")
        implementation("androidx.annotation:annotation-experimental:1.4.1")
        implementation("org.jetbrains.kotlin:kotlin-stdlib:1.8.22")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-common:1.8.22")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:1.8.22")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:1.8.20")
        implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")
        implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
    }
}
