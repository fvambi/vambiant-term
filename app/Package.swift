// swift-tools-version: 6.0
// The Swift shell of Vambiant Term (docs/02 §5, ADR-0002). SwiftPM only —
// never an .xcodeproj. The Rust core is linked as a static library built by
// `mise run ffi:staticlib` (see scripts/ci/swift-build.sh for why that exact
// command); `CVambiantTerm` is the cbindgen header as a Clang module.
import PackageDescription

let rustLibDir = "\(Context.packageDirectory)/../target/release"

let package = Package(
    name: "VambiantTerm",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "CVambiantTerm", path: "Sources/CVambiantTerm"),
        .executableTarget(
            name: "VambiantTerm",
            dependencies: ["CVambiantTerm"],
            path: "Sources/VambiantTerm",
            swiftSettings: [.swiftLanguageMode(.v6)],
            linkerSettings: [
                .unsafeFlags(["-L", rustLibDir]),
                .linkedLibrary("vambiant_term"),
                .linkedLibrary("iconv"),
                .linkedFramework("AppKit"),
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("CoreText"),
            ]
        ),
        .testTarget(
            name: "VambiantTermTests",
            dependencies: ["VambiantTerm"],
            path: "Tests/VambiantTermTests",
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
    ]
)
