// swift-tools-version:5.9
//
// Moodflow macOS — SwiftPM 工程 (PR-A M1 + M2).
//
// 目标:
//   - MoodflowKit   : 二进制协议 codec + UDS BridgeClient (Pure Swift, 跨 target 共享)
//   - MoodflowSmoke : 命令行 smoke 工具, 连真实 bridge 收发帧 (用于无 IMKit 时的协议验证)
//   - MoodflowApp   : IMKit 输入法主体, swift build 出来的二进制由 scripts_mac/build/app.sh
//                       打成 .app bundle (含 Info.plist + Resources), 系统通过 .app 加载
//
// 注: SwiftPM 自身不支持直接产出 .app bundle, build script 负责后处理.
import PackageDescription

let package = Package(
    name: "Moodflow",
    platforms: [.macOS(.v12)],
    products: [
        .library(name: "MoodflowKit", targets: ["MoodflowKit"]),
        .executable(name: "moodflow-smoke", targets: ["MoodflowSmoke"]),
        .executable(name: "moodflow-input-app", targets: ["MoodflowApp"]),
        .executable(name: "moodflow-input-demo", targets: ["MoodflowDemo"]),
    ],
    targets: [
        .target(
            name: "MoodflowKit",
            path: "Sources/MoodflowKit"
        ),
        .executableTarget(
            name: "MoodflowSmoke",
            dependencies: ["MoodflowKit"],
            path: "Sources/MoodflowSmoke"
        ),
        .executableTarget(
            name: "MoodflowApp",
            dependencies: ["MoodflowKit"],
            path: "Sources/MoodflowApp",
            // Info.plist / entitlements 不参与 swift build, 由 build_macos_app.sh
            // 在拼 .app bundle 时复制. exclude 避免 SwiftPM 把它们当成源文件.
            exclude: [
                "Resources/Info.plist",
                "Resources/Moodflow.entitlements",
                "Resources/zh-Hans.lproj",
                "Resources/en.lproj",
            ],
            linkerSettings: [
                .linkedFramework("InputMethodKit"),
                .linkedFramework("Cocoa"),
            ]
        ),
        .executableTarget(
            name: "MoodflowDemo",
            dependencies: ["MoodflowKit"],
            path: "Sources/MoodflowDemo",
            // M3 候选框开发期 AppKit demo: 绕开 IMKit 签名/注册墙, 直接 mmap SHM
            // (/Moodflow_SHM) 把 Go 端 hostrender 出的 BGRA 帧贴到 NSWindow,
            // 让候选框 UI 迭代 < 5 秒闭环 (swift run moodflow-input-demo)。
            linkerSettings: [
                .linkedFramework("Cocoa"),
            ]
        ),
        .testTarget(
            name: "MoodflowKitTests",
            dependencies: ["MoodflowKit"],
            path: "Tests/MoodflowKitTests"
        ),
    ]
)
