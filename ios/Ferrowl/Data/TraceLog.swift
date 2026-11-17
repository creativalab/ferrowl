import Foundation

/// Steps of slow paths (an account switch, a message opened from a
/// notification), with the time, in `Documents/trace.log` on the phone:
/// read with `xcrun devicectl device copy from --domain-type
/// appDataContainer --domain-identifier com.ferrowl.app --source
/// Documents/trace.log --destination trace.log`. Only what the app does,
/// never addresses or message contents beyond the account in use.
enum TraceLog {
    private static let queue = DispatchQueue(label: "com.ferrowl.trace")
    private static let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("trace.log")
    /// Kept small: the oldest half goes beyond this.
    private static let limit = 256 * 1024

    private static let clock: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm:ss.SSS"
        return f
    }()

    static func note(_ text: String) {
        #if DEBUG
        if Demo.isOn { return }
        #endif
        let line = "\(clock.string(from: Date())) \(text)\n"
        queue.async {
            var data = (try? Data(contentsOf: url)) ?? Data()
            data.append(Data(line.utf8))
            if data.count > limit { data = data.suffix(limit / 2) }
            try? data.write(to: url, options: .atomic)
        }
    }
}
