import XCTest
@testable import PatchwrightCore

final class CodexPresentationTests: XCTestCase {
    func testOrderedEventsCoalesceStreamingContentAndRetainUnknownEvents() throws {
        let data = Data(#"""
        [
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":1,"kind":"userMessage","summary":"sent","threadId":"thread-1","turnId":"turn-1","itemId":"message-1","content":"Please implement.","occurredAt":"2026-07-14T08:00:00Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":2,"kind":"textDelta","summary":"stream","threadId":"thread-1","turnId":"turn-1","itemId":"agent-1","content":"Hello ","occurredAt":"2026-07-14T08:00:01Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":3,"kind":"textDelta","summary":"stream","threadId":"thread-1","turnId":"turn-1","itemId":"agent-1","content":"world","occurredAt":"2026-07-14T08:00:02Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":4,"kind":"reasoningDelta","summary":"reasoning","threadId":"thread-1","turnId":"turn-1","itemId":"reason-1","content":"Checked contract","occurredAt":"2026-07-14T08:00:03Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":5,"kind":"commandOutputDelta","summary":"command","threadId":"thread-1","turnId":"turn-1","itemId":"command-1","content":"tests passed","occurredAt":"2026-07-14T08:00:04Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":6,"kind":"fileChangeDelta","summary":"file","threadId":"thread-1","turnId":"turn-1","itemId":"file-1","content":"M App.swift","occurredAt":"2026-07-14T08:00:05Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":7,"kind":"futureEvent","summary":"future","content":"retained","occurredAt":"2026-07-14T08:00:06Z"},
          {"taskId":"11111111-1111-1111-1111-111111111111","processGeneration":"22222222-2222-2222-2222-222222222222","sequence":8,"kind":"turnCompleted","summary":"done","threadId":"thread-1","turnId":"turn-1","content":"completed","occurredAt":"2026-07-14T08:00:07Z"}
        ]
        """#.utf8)
        let events = try JSONDecoder.patchwright.decode([CodexEvent].self, from: data)
        let transcript = CodexTranscript(events: events)

        XCTAssertEqual(transcript.cursor, 8)
        XCTAssertEqual(transcript.items.map(\.kind), [
            .operatorMessage, .agentMessage, .reasoning, .command, .fileChange, .unknown("futureEvent"), .status,
        ])
        XCTAssertEqual(transcript.items[1].content, "Hello world")
        XCTAssertEqual(transcript.items[2].content, "Checked contract")
        XCTAssertEqual(transcript.items[3].content, "tests passed")
        XCTAssertEqual(transcript.items[4].content, "M App.swift")
        XCTAssertEqual(transcript.items[5].content, "retained")
    }

    func testReconnectCursorLongContentAndSendSteerStatesDecode() throws {
        let longContent = String(repeating: "a", count: 64 * 1_024)
        let event = CodexEvent(
            taskId: UUID(),
            processGeneration: UUID(),
            sequence: 42,
            kind: .textDelta,
            summary: "stream",
            threadId: "thread",
            turnId: "turn",
            itemId: "item",
            content: longContent,
            occurredAt: Date()
        )
        XCTAssertEqual(CodexTranscript(events: [event]).cursor, 42)
        XCTAssertEqual(CodexTranscript(events: [event]).items.first?.content.count, 64 * 1_024)

        let data = Data(#"{"taskId":"11111111-1111-1111-1111-111111111111","state":"ready","processGeneration":"22222222-2222-2222-2222-222222222222","accountState":"signedOut","threadId":"thread","turnId":"turn","lastSequence":42,"canStart":false,"canSend":true,"canSteer":false}"#.utf8)
        let status = try JSONDecoder.patchwright.decode(CodexRuntimeStatus.self, from: data)
        XCTAssertTrue(status.canSend)
        XCTAssertFalse(status.canSteer)
        XCTAssertFalse(status.canStart)
    }

    func testRuntimeApprovalDecodesExactSingleUseBoundary() throws {
        let data = Data(#"{"id":"33333333-3333-3333-3333-333333333333","taskId":"11111111-1111-1111-1111-111111111111","class":"codexRuntime","requestId":"command-request","processGeneration":"22222222-2222-2222-2222-222222222222","threadId":"thread-1","turnId":"turn-1","itemId":"item-1","kind":"command","reason":"Run tests","command":"swift test","cwd":"/tmp/worktree","grantRoot":null,"state":"pending","createdAt":"2026-07-14T08:00:00Z","expiresAt":"2026-07-14T08:10:00Z","decidedAt":null}"#.utf8)
        let approval = try JSONDecoder.patchwright.decode(CodexRuntimeApproval.self, from: data)
        XCTAssertEqual(approval.class, "codexRuntime")
        XCTAssertEqual(approval.kind, .command)
        XCTAssertEqual(approval.state, .pending)
        XCTAssertEqual(approval.requestId, .string("command-request"))
        XCTAssertEqual(approval.command, "swift test")
    }
}
