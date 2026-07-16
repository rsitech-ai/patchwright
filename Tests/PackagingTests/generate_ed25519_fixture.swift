import CryptoKit
import Foundation

guard CommandLine.arguments.count == 3 else {
    exit(64)
}

let privateKey = Curve25519.Signing.PrivateKey()
let archive = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let template = try String(contentsOfFile: CommandLine.arguments[2], encoding: .utf8)
let archiveSignature = try privateKey.signature(for: archive).base64EncodedString()
let feed = template.replacingOccurrences(of: "__ARCHIVE_SIGNATURE__", with: archiveSignature)
let feedSignature = try privateKey.signature(for: Data(feed.utf8)).base64EncodedString()
let output: [String: String] = [
    "public_key": privateKey.publicKey.rawRepresentation.base64EncodedString(),
    "archive_signature": archiveSignature,
    "feed_signature": feedSignature,
]
let data = try JSONSerialization.data(withJSONObject: output, options: [.sortedKeys])
FileHandle.standardOutput.write(data)
