#!/usr/bin/env swift

import CryptoKit
import Foundation

guard CommandLine.arguments.count == 4 || CommandLine.arguments.count == 5 else {
    FileHandle.standardError.write(Data("usage: verify_ed25519.swift PUBLIC_KEY_BASE64 SIGNATURE_BASE64 FILE [SIGNED_LENGTH]\n".utf8))
    exit(64)
}

do {
    guard let publicBytes = Data(base64Encoded: CommandLine.arguments[1]), publicBytes.count == 32,
          let signature = Data(base64Encoded: CommandLine.arguments[2]), signature.count == 64 else {
        throw NSError(domain: "PatchwrightEd25519", code: 65, userInfo: [NSLocalizedDescriptionKey: "invalid Ed25519 key or signature encoding"])
    }
    let file = URL(fileURLWithPath: CommandLine.arguments[3])
    let fullData = try Data(contentsOf: file, options: [.mappedIfSafe])
    let signedData: Data
    if CommandLine.arguments.count == 5 {
        guard let length = Int(CommandLine.arguments[4]), length >= 0, length <= fullData.count else {
            throw NSError(domain: "PatchwrightEd25519", code: 65, userInfo: [NSLocalizedDescriptionKey: "invalid signed length"])
        }
        signedData = Data(fullData.prefix(length))
    } else {
        signedData = fullData
    }
    let publicKey = try Curve25519.Signing.PublicKey(rawRepresentation: publicBytes)
    guard publicKey.isValidSignature(signature, for: signedData) else {
        throw NSError(domain: "PatchwrightEd25519", code: 65, userInfo: [NSLocalizedDescriptionKey: "Ed25519 signature verification failed"])
    }
} catch {
    FileHandle.standardError.write(Data("Ed25519 verification failed: \(error.localizedDescription)\n".utf8))
    exit(65)
}
