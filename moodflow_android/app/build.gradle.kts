import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// 版本取 moodflow_android/VERSION（XSIME 吸收计划 §4.1：主次版本号与 docs/VERSION 一致，
// 补丁号可独立递增；标签 android-vX.Y.Z）。versionCode = 主×1e6 + 次×1e3 + 补丁。
val appVersion: String = rootProject.file("VERSION").readText().trim()
val appVersionCode: Int = appVersion.split(".").let { parts ->
    fun at(i: Int) = parts.getOrNull(i)?.takeWhile(Char::isDigit)?.toIntOrNull() ?: 0
    at(0) * 1_000_000 + at(1) * 1_000 + at(2)
}

// cargo-ndk 产出的 .so 与从仓库 data/ 拷来的出厂数据都放在 build/ 下，不入库。
val rustJniDir: File = layout.buildDirectory.dir("rustJniLibs").get().asFile
val generatedAssetsDir: File = layout.buildDirectory.dir("generated/moodflowAssets").get().asFile

android {
    namespace = "com.moodflow.ime"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.moodflow.ime"
        minSdk = 26
        targetSdk = 35
        versionCode = appVersionCode
        versionName = appVersion
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir(rustJniDir)
            assets.srcDir(generatedAssetsDir)
        }
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":imelogic"))
    testImplementation("junit:junit:4.13.2")
}

// ── Rust：cargo-ndk 交叉编译 libmoodflow_android.so ─────────────────────────
//
// 需要 cargo、cargo-ndk、rustup 目标（aarch64-linux-android / x86_64-linux-android）与
// ANDROID_NDK_HOME。只想编 Kotlin 时加 -Pmoodflow.skipRust=true（APK 里就没有 .so，
// 键盘会退化成直接上屏）。-Pmoodflow.abis=arm64-v8a 可只编一个 ABI。
val rustAbis: List<String> = (providers.gradleProperty("moodflow.abis").orNull ?: "arm64-v8a,x86_64")
    .split(",").map { it.trim() }.filter { it.isNotEmpty() }
val skipRust: Boolean = providers.gradleProperty("moodflow.skipRust").orNull == "true"

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    group = "build"
    description = "用 cargo-ndk 编译 libmoodflow_android.so"
    workingDir = rootProject.file("rust")
    val cmd = mutableListOf("cargo", "ndk")
    rustAbis.forEach { cmd += listOf("-t", it) }
    cmd += listOf("-P", "26", "-o", rustJniDir.absolutePath, "build", "--release", "-p", "moodflow-android")
    commandLine(cmd)
    // 增量交给 cargo 自己判断
    outputs.upToDateWhen { false }
}

// ── 出厂数据 → APK assets/data（首次启动由 DataInstaller 解到 filesDir/data）──
// 优先用完整运行时数据：`scripts/dev.sh gd` 组装的 build_dev/data（下载的拼音/英文词库、生成的
// 五笔库、OpenCC 表、拼音反查表都在里面，与 Windows 安装包同一份）。可用 -Pmoodflow.dataDir=… 指定。
// 都没有时退回仓库根 data/——那里只有配置与方案定义，**没有词库，打不出中文候选**，构建会给出警告。
val moodflowDataDir: File = run {
    val explicit = (project.findProperty("moodflow.dataDir") as String?)?.let { file(it) }
    val assembled = rootProject.file("../build_dev/data")
    when {
        explicit != null -> explicit
        assembled.resolve("schemas/pinyin/rime_frost.dict.yaml").exists() -> assembled
        else -> {
            logger.warn("moodflow: 没找到组装好的词库数据（先跑 scripts/dev.sh gd），APK 里将没有词库")
            rootProject.file("../data")
        }
    }
}
val copyMoodflowData = tasks.register<Sync>("copyMoodflowData") {
    description = "把出厂数据（含词库）拷进 APK assets"
    from(moodflowDataDir)
    into(File(generatedAssetsDir, "data"))
}

// 除了挂在 preBuild 上，再显式挂到真正消费这两个目录的合并任务上：Gradle 8 会拒绝
// 「用了别的任务的输出却没声明依赖」的任务，不能只靠 preBuild 的传递顺序。
val assetConsumers = Regex("(merge|package)\\w*Assets")
val jniConsumers = Regex("merge\\w*JniLibFolders")
tasks.configureEach {
    if (name == "preBuild" || assetConsumers.matches(name)) {
        dependsOn(copyMoodflowData)
    }
    if (!skipRust && (name == "preBuild" || jniConsumers.matches(name))) {
        dependsOn(cargoBuild)
    }
}
