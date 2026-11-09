#if DEBUG
import Foundation
import QuartzCore

/// Development builds only: measures how long the main thread stalls while
/// messages are opened and closed, without anyone tapping. Started from the
/// command line:
///
///     xcrun devicectl device process launch --terminate-existing \
///       --environment-variables '{"FERROWL_AUTOTEST":"1"}' \
///       --device <UDID> com.ferrowl.app
///
/// It opens and closes the first messages of the inbox and writes each stall
/// over 50 ms to `Documents/perf.log` in the app's container, then a
/// summary (`xcrun devicectl device copy from --domain-type appDataContainer
/// --domain-identifier com.ferrowl.app --source Documents/perf.log ...`).
@MainActor
enum PerfProbe {
    private static var last = CACurrentMediaTime()
    private static var phase = "idle"
    private static var timer: Timer?
    private static var stalls: [String: [Double]] = [:]

    static var enabled: Bool { ProcessInfo.processInfo.environment["FERROWL_AUTOTEST"] != nil }

    private static var file: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("perf.log")
    }

    static func log(_ line: String) {
        let text = String(format: "%.3f ", CACurrentMediaTime()) + line + "\n"
        if let handle = try? FileHandle(forWritingTo: file) {
            handle.seekToEndOfFile()
            handle.write(Data(text.utf8))
            try? handle.close()
        } else {
            try? Data(text.utf8).write(to: file)
        }
    }

    /// A main-thread watchdog: a timer that should fire every frame; a late
    /// one means the main thread was busy that long.
    private static func watch() {
        last = CACurrentMediaTime()
        let t = Timer(timeInterval: 0.008, repeats: true) { _ in
            MainActor.assumeIsolated {
                let now = CACurrentMediaTime()
                let gap = now - last
                last = now
                if gap > 0.05 {
                    stalls[phase, default: []].append(gap)
                    log("stall \(Int(gap * 1000)) ms during \(phase)")
                }
            }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    static func autotest(_ model: MailboxModel) async {
        guard enabled else { return }
        try? FileManager.default.removeItem(at: file)
        log("start rows=\(model.rows.count) messages=\(model.messages.count)")
        watch()
        try? await Task.sleep(for: .seconds(4))
        let rows = Array(model.rows.prefix(5))
        for row in rows {
            phase = "open"
            model.open = row
            try? await Task.sleep(for: .seconds(3))
            phase = "close"
            model.open = nil
            try? await Task.sleep(for: .seconds(2.5))
        }
        phase = "idle"
        for (p, gaps) in stalls.sorted(by: { $0.key < $1.key }) {
            log("summary \(p): count=\(gaps.count) max=\(Int((gaps.max() ?? 0) * 1000)) ms total=\(Int(gaps.reduce(0, +) * 1000)) ms")
        }
        log("done")
    }
}

#endif
