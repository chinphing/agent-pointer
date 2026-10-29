// 测 Cmd+V 的 keydown→keyup 间隔是否必要。
//
// 靶子：TextEdit 新建文档。每轮：写唯一标记到剪贴板 → 清空文档 →
// 按 Cmd+V（keydown 与 keyup 间隔 = 被测值）→ 读回文档内容比对。
//
// 用 CGEvent 直接发键，不经过 enigo，避免 TIS 崩溃干扰测量。

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

/// 返回 true = 粘贴成功（文档内容 == 标记）
func trial(mark: String, gapMs: UInt32) -> Bool {
    setClipboard(mark)
    usleep(10_000)
    clearDoc()

    post(kcV, down: true, flags: .maskCommand)
    if gapMs > 0 { usleep(gapMs * 1000) }
    post(kcV, down: false, flags: .maskCommand)
    usleep(120_000)  // 等粘贴落地

    return readTextEdit() == mark
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

let gaps: [UInt32] = [0, 5, 10, 20]
let rounds = 30

print("档位(ms)\t成功\t总数\t成功率")
for gap in gaps {
    var ok = 0
    for i in 0..<rounds {
        let mark = "probe-\(gap)-\(i)-x"
        if trial(mark: mark, gapMs: gap) { ok += 1 }
    }
    let rate = Double(ok) / Double(rounds) * 100
    print("\(gap)\t\(ok)\t\(rounds)\t\(String(format: "%.0f", rate))%")
}
