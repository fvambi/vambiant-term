// Entry point. `@main` on a type (not main.swift) keeps the module
// importable from the test target.

import AppKit
import CVambiantTerm

@main
enum VambiantTermApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private(set) var daemon: DaemonClient!
    private(set) var renderer: GridRenderer!
    private var windows: [TerminalWindowController] = []
    private var probe: LatencyProbe?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let abi = vt_ffi_abi_version()
        guard abi == UInt32(VtABI_VERSION) else {
            fatal("libvambiant_term ABI \(abi) does not match the header's \(VtABI_VERSION); rebuild with `mise run ffi:staticlib`")
        }
        let socket = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SOCKET"] ?? DaemonClient.defaultSocket()
        daemon = DaemonClient(socket: socket)
        do {
            _ = try daemon.ensureRunning()
        } catch {
            fatal("\(error)")
        }
        guard let device = MTLCreateSystemDefaultDevice() else { fatal("no Metal device") }
        do {
            renderer = try GridRenderer(
                device: device,
                fonts: FontSet(),
                theme: .vambiantDark,
                scale: NSScreen.main?.backingScaleFactor ?? 2
            )
        } catch {
            fatal("\(error)")
        }
        installMenu()
        newWindow(tabbedWith: nil)
        NSApp.activate(ignoringOtherApps: true)
        if let n = ProcessInfo.processInfo.environment["VAMBIANT_TERM_LATENCY_PROBE"].flatMap(Int.init),
           let pane = windows.first?.container.focused {
            probe = LatencyProbe(pane: pane, keystrokes: n)
            probe?.start()
        }
        if let shot = ProcessInfo.processInfo.environment["VAMBIANT_TERM_SCREENSHOT"],
           let pane = windows.first?.container.focused {
            // Something worth looking at: attributes, colours, CJK, emoji.
            let demo = "clear; printf '\\e[1mbold\\e[0m \\e[3mitalic\\e[0m \\e[4munderline\\e[0m "
                + "\\e[31mred\\e[0m \\e[42m bg \\e[0m \\e[38;5;208m256\\e[0m \\e[38;2;122;162;247mrgb\\e[0m "
                + "世界 😀\\n'; ls -la | head -6\r"
            Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { _ in
                MainActor.assumeIsolated { _ = pane.viewer?.send(text: demo) }
            }
            Timer.scheduledTimer(withTimeInterval: 3.5, repeats: false) { _ in
                MainActor.assumeIsolated { pane.view.captureNext(to: shot) }
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    @discardableResult
    func newWindow(tabbedWith existing: NSWindow?) -> TerminalWindowController {
        let controller = TerminalWindowController(daemon: daemon, renderer: renderer)
        windows.append(controller)
        if let existing, let w = controller.window {
            existing.addTabbedWindow(w, ordered: .above)
        }
        controller.showWindow(nil)
        controller.window?.makeKeyAndOrderFront(nil)
        return controller
    }

    func forget(_ controller: TerminalWindowController) {
        windows.removeAll { $0 === controller }
    }

    @objc func newWindowAction(_ sender: Any?) {
        newWindow(tabbedWith: nil)
    }

    @objc func newTabAction(_ sender: Any?) {
        newWindow(tabbedWith: NSApp.keyWindow)
    }

    private func installMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(
            withTitle: "About Vambiant Term",
            action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)),
            keyEquivalent: ""
        )
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Vambiant Term", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let shell = NSMenuItem()
        let shellMenu = NSMenu(title: "Shell")
        shellMenu.addItem(withTitle: "New Window", action: #selector(newWindowAction(_:)), keyEquivalent: "n")
        shellMenu.addItem(withTitle: "New Tab", action: #selector(newTabAction(_:)), keyEquivalent: "t")
        shellMenu.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        shell.submenu = shellMenu
        main.addItem(shell)

        let edit = NSMenuItem()
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Copy", action: #selector(MetalGridView.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(MetalGridView.paste(_:)), keyEquivalent: "v")
        edit.submenu = editMenu
        main.addItem(edit)

        let window = NSMenuItem()
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.miniaturize(_:)), keyEquivalent: "m")
        window.submenu = windowMenu
        main.addItem(window)
        NSApp.mainMenu = main
        NSApp.windowsMenu = windowMenu
    }

    private func fatal(_ message: String) -> Never {
        FileHandle.standardError.write(Data("vambiant-term: \(message)\n".utf8))
        let alert = NSAlert()
        alert.messageText = "Vambiant Term cannot start"
        alert.informativeText = message
        alert.runModal()
        exit(2)
    }
}
