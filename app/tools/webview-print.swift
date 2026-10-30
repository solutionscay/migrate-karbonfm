// Spike S-02: print a page to PDF through WKWebView the way the desktop app does, without the print dialog.
//
// wry's WebView::print on macOS (wry 0.57, src/wkwebview/mod.rs) takes NSPrintInfo.shared, sets its margins
// from PrintOptions (all 0 by default), calls WKWebView.printOperation(with:) and runs it modally on the window.
// This tool does the same, except that the job goes to a PDF file instead of the dialog, so the result equals
// "Save as PDF" in the dialog with its default settings.
//
// Usage: swift tools/webview-print.swift <url> <out.pdf> [margin-pt]
import AppKit
import WebKit

let args = CommandLine.arguments
guard args.count >= 3, let url = URL(string: args[1]) else {
    FileHandle.standardError.write("usage: webview-print.swift <url> <out.pdf> [margin-pt]\n".data(using: .utf8)!)
    exit(2)
}
let out = URL(fileURLWithPath: args[2])
let margin = args.count > 3 ? CGFloat(Double(args[3]) ?? 0) : 0

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

final class Loader: NSObject, WKNavigationDelegate {
    let window: NSWindow
    let webView: WKWebView
    init(url: URL) {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900), styleMask: [.titled],
                          backing: .buffered, defer: false)
        webView = WKWebView(frame: window.contentView!.bounds)
        super.init()
        window.contentView = webView
        webView.navigationDelegate = self
        webView.load(URLRequest(url: url))
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // Let web fonts finish loading before printing.
        webView.evaluateJavaScript("document.fonts.ready.then(() => true)") { _, _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { self.print() }
        }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        fail(error)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        fail(error)
    }

    func fail(_ error: Error) {
        FileHandle.standardError.write("load failed: \(error)\n".data(using: .utf8)!)
        exit(1)
    }

    func print() {
        let info = NSPrintInfo.shared.copy() as! NSPrintInfo
        info.topMargin = margin
        info.rightMargin = margin
        info.bottomMargin = margin
        info.leftMargin = margin
        info.jobDisposition = .save
        info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = out
        let op = webView.printOperation(with: info)
        op.showsPrintPanel = false
        op.showsProgressPanel = false
        // As wry: run modally for the window; the delegate callback ends the tool.
        op.runModal(for: window, delegate: self, didRun: #selector(done(_:success:contextInfo:)), contextInfo: nil)
    }

    @objc func done(_ op: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?) {
        let paper = op.printInfo.paperSize
        Swift.print("\(success ? "saved" : "failed") \(out.path) paper \(Int(paper.width))x\(Int(paper.height)) pt")
        exit(success ? 0 : 1)
    }
}

let loader = Loader(url: url)
app.run()
