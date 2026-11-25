pluginManagement {
    repositories {
        // 本机到 google()/mavenCentral() 的 TLS 握手会被中断;用国内镜像
        // (腾讯云)取 AndroidX 与 Kotlin 制品 —— 内容与官方仓库一致。
        maven("https://mirrors.cloud.tencent.com/nexus/repository/maven-public/")
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        maven("https://mirrors.cloud.tencent.com/nexus/repository/maven-public/")
        google()
        mavenCentral()
    }
}

rootProject.name = "yu-android-shell"
include(":app")
// 轻量 Tyranor-Next 运行时模块:只有入口 + 游戏页,不含游戏库/配置页/Compose。
include(":runtime")
