// swift-tools-version: 6.0
// swift-tools-version: 5.9

import Foundation
import PackageDescription

// Bepaal de absolute map waar dit Package.swift bestand in staat
let packageDir = URL(fileURLWithPath: #file).deletingLastPathComponent().path

let package = Package(
    name: "hyprPad",
    platforms: [
        .iOS("26")
    ],
    products: [
        .library(
            name: "hyprPadClient",
            targets: ["hyprPadClient"]
        )
    ],
    targets: [
        .target(
            name: "CRustCore",
            path: "Sources/CRustCore",
            publicHeadersPath: "include"
        ),
        .target(
            name: "hyprPadClient",
            dependencies: ["CRustCore"],
            path: "Sources/hyprPadClient",
            linkerSettings: [
                // Geef direct het absolute pad naar het bestand, niet de map!
                .unsafeFlags(["/home/alberic/hyprPad/swift/Sources/CRustCore/lib/librust_core.a"])
            ]
        ),
    ]
)
