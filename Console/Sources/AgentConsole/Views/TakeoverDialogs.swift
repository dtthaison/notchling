import ConsoleCore
import SwiftUI

/// The confirmations around "Tiếp quản", in one place: the first confirmation (stop the original or just resume),
/// the second one for a session that is mid-turn, and the error alert.
struct TakeoverDialogs: ViewModifier {
    @Bindable var model: ConsoleModel

    private var firstBinding: Binding<Bool> {
        Binding(get: { model.confirmingTakeover != nil }, set: { if !$0 { model.confirmingTakeover = nil } })
    }

    private var busyBinding: Binding<Bool> {
        Binding(get: { model.confirmingBusyStop != nil }, set: { if !$0 { model.confirmingBusyStop = nil } })
    }

    private var errorBinding: Binding<Bool> {
        Binding(get: { model.takeoverError != nil }, set: { if !$0 { model.takeoverError = nil } })
    }

    private func stoppable(_ session: Session) -> Bool {
        if case .stoppable? = Takeover.state(for: session, paths: model.paths) { return true }
        return false
    }

    func body(content: Content) -> some View {
        content
            .confirmationDialog("Tiếp quản session này?", isPresented: firstBinding, presenting: model.confirmingTakeover) { session in
                if stoppable(session) {
                    Button("Dừng bản gốc và tiếp quản", role: .destructive) { model.confirmTakeover(stopOriginal: true) }
                } else {
                    Button("Tiếp quản") { model.confirmTakeover(stopOriginal: false) }
                }
                Button("Huỷ", role: .cancel) {}
            } message: { session in
                if stoppable(session) {
                    Text("Dừng session trên Mac (Claude desktop / VS Code) và mở lại trong Console để nhắn từ điện thoại?")
                } else {
                    Text("Console sẽ chạy \"claude --resume\" cho \"\(session.name)\" trong Terminal của Console, cùng thư mục. Session gốc đã đóng nên không có hai nơi cùng ghi. Nếu Claude hỏi gì, bạn tự trả lời trong Terminal; Console không tự bấm.")
                }
            }
            .confirmationDialog("Session đang chạy", isPresented: busyBinding, presenting: model.confirmingBusyStop) { _ in
                Button("Vẫn dừng và tiếp quản", role: .destructive) { model.confirmBusyStop() }
                Button("Huỷ", role: .cancel) {}
            } message: { _ in
                Text("Session đang chạy — lượt hiện tại sẽ bị dừng giữa chừng.")
            }
            .alert("Không tiếp quản được", isPresented: errorBinding) {
                Button("Đóng", role: .cancel) {}
            } message: {
                Text(model.takeoverError ?? "")
            }
    }
}
