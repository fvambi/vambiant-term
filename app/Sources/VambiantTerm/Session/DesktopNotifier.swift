// Notification Center (docs/06 §8): only when running from the bundle
// (the framework refuses otherwise), authorisation asked once, one
// thread per session so macOS groups them. Actions come with the
// actionable-approval work; the daemon posts approvals itself.

import Foundation
import UserNotifications

final class DesktopNotifier: @unchecked Sendable {
    private let available = Bundle.main.bundleIdentifier != nil
    private var asked = false

    func post(_ note: Note) {
        guard available else { return }
        let center = UNUserNotificationCenter.current()
        if !asked {
            asked = true
            center.requestAuthorization(options: [.alert, .sound]) { _, error in
                if let error {
                    NSLog("notifications: authorisation failed: %@", "\(error)")
                }
            }
        }
        let content = UNMutableNotificationContent()
        content.title = note.title
        content.body = note.body
        if let session = note.session {
            content.threadIdentifier = session
        }
        let request = UNNotificationRequest(identifier: note.id, content: content, trigger: nil)
        center.add(request) { error in
            if let error {
                NSLog("notifications: %@", "\(error)")
            }
        }
    }
}
