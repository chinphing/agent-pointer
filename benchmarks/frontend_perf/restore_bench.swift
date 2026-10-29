// 测「粘贴后多久恢复剪贴板」是否必要。
//
// 靶子：TextEdit 新建文档。每轮：
//   1. 剪贴板写入旧值 OLDMARK
//   2. 清空文档
//   3. 剪贴板写入本轮标记 MARK
//   4. 按 Cmd+V
//   5. 等 restoreDelay 后把剪贴板恢复成 OLDMARK   ← 被测时机
//   6. 读回文档，比对 == MARK
//
// 若 restoreDelay 太小，目标应用还没读完剪贴板就被换掉，粘贴结果会是
// OLDMARK 或空 —— 这一步就能暴露。

import Cocoa
import CoreGraphics

let kcA: CGKeyCode = 0x00
let kcV: CGKeyCode = 0x09
let kcDelete: CGKeyCode = 0x33

func post(_ key: CGKeyCode, down: Bool, flags: CGEventFlags) {
    let src = CGEventSource(stateID: .hidSystemState)
    guard let e = CGEvent(keyboardEventSource: src, virtualKey: key, keyDown: down) else { return }
    e.flags = flags
    e.post(tap: .cghidEventTap)
}

func setClipboard(_ s: String) {
    let pb = NSPasteboard.general
    pb.clearContents()
    pb.setString(s, forType: .string)
}

func readTextEdit() -> String {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
    p.arguments = ["-e", "tell application \"TextEdit\" to get text of document 1"]
    let out = Pipe()
    p.standardOutput = out
    p.standardError = Pipe()
    try? p.run()
    p.waitUntilExit()
    let data = out.fileHandleForReading.readDataToEndOfFile()
    return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
}

func clearDoc() {
    post(kcA, down: true, flags: .maskCommand)
    post(kcA, down: false, flags: .maskCommand)
    usleep(30_000)
    post(kcDelete, down: true, flags: [])
    post(kcDelete, down: false, flags: [])
    usleep(30_000)
}

enum Outcome: String {
    case ok = "OK"
    case stale = "读到旧值"
    case empty = "空"
    case other = "其他"
}

func trial(mark: String, old: String, restoreDelayMs: UInt32) -> Outcome {
    setClipboard(old)
    clearDoc()

    setClipboard(mark)
    usleep(5_000)

    post(kcV, down: true, flags: .maskCommand)
    post(kcV, down: false, flags: .maskCommand)

    if restoreDelayMs > 0 { usleep(restoreDelayMs * 1000) }
    setClipboard(old)          // ← 恢复旧剪贴板

    usleep(120_000)            // 等落定再读
    let got = readTextEdit()
    if got == mark { return .ok }
    if got == old { return .stale }
    if got.isEmpty { return .empty }
    return .other
}

// MARK: - main

let script = """
tell application "TextEdit"
    activate
    if (count of documents) = 0 then make new document
end tell
"""
var err: NSDictionary?
NSAppleScript(source: script)?.executeAndReturnError(&err)
usleep(600_000)

let delays: [UInt32] = [30, 50, 100, 200, 500]
let rounds = 30
let old = "OLDMARK-sentinel"

print("恢复延迟(ms)\tOK\t旧值\t空\t其他")
for d in delays {
    var ok = 0, stale = 0, empty = 0, other = 0
    for i in 0..<rounds {
        let mark = "MARK-\(d)-\(i)-z"
        switch trial(mark: mark, old: old, restoreDelayMs: d) {
        case .ok: ok += 1
        case .stale: stale += 1
        case .empty: empty += 1
        case .other: other += 1
        }
    }
    print("\(d)\t\(ok)\t\(stale)\t\(empty)\t\(other)")
}
