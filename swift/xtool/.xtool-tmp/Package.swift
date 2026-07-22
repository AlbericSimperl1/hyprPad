// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "hyprPadClient-Builder",
    platforms: [
        .iOS("26"),
    ],
    dependencies: [
        .package(name: "RootPackage", path: "../.."),
    ],
    targets: [
        .executableTarget(
    name: "hyprPadClient-App",
    dependencies: [
        .product(name: "hyprPadClient", package: "RootPackage"),
    ],
    linkerSettings: [
    .unsafeFlags([
        "-Xlinker", "-rpath", "-Xlinker", "@executable_path/Frameworks",
    ]),
]
)
    ]
)
