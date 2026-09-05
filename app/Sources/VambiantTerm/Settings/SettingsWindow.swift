// Settings (⌘,): a SwiftUI form hosted in an AppKit window.

import AppKit
import SwiftUI

@MainActor
final class SettingsWindowController: NSWindowController {
    let model: ConfigModel

    init(model: ConfigModel) {
        self.model = model
        let host = NSHostingController(rootView: SettingsView(model: model))
        let window = NSWindow(contentViewController: host)
        window.title = "Settings"
        window.setContentSize(NSSize(width: 860, height: 560))
        window.styleMask = [.titled, .closable, .resizable, .miniaturizable]
        window.tabbingMode = .disallowed
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.center()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    /// Diagnostics: the window's content as a PNG.
    func capture(to path: String) {
        guard let view = window?.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        if let data = rep.representation(using: NSBitmapImageRep.FileType.png, properties: [:]) {
            try? data.write(to: URL(fileURLWithPath: path))
            NSLog("screenshot: wrote %@", path)
        }
    }
}
