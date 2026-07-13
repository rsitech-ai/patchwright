import PatchwrightCore
import SwiftUI

struct TimestampText: View {
    let date: Date?

    var body: some View {
        if let date {
            let presentation = TimestampPresentation(date: date)
            Text(presentation.relative)
                .help(presentation.exact)
                .accessibilityLabel("\(presentation.relative), \(presentation.exact)")
        } else {
            Text("—")
                .foregroundStyle(.tertiary)
                .accessibilityLabel("Not available")
        }
    }
}
