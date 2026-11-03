import ConsoleCore
import SwiftUI

struct TopBar: View {
    @Bindable var model: ConsoleModel
    var searchFocused: FocusState<Bool>.Binding

    private var needsInputCount: Int { model.sessions.filter { $0.state == .needsInput }.count }

    var body: some View {
        HStack(spacing: Space.md) {
            HStack(spacing: Space.sm) {
                Image(systemName: "rectangle.3.group").font(.system(size: 13)).foregroundStyle(Token.muted)
                    .accessibilityHidden(true)
                Text("Agent Console").font(TypeToken.bodyMdStrong).foregroundStyle(Token.onSurface)
            }
            Spacer(minLength: Space.md)
            if needsInputCount > 0 {
                HStack(spacing: 6) {
                    InputBadge(text: "\(needsInputCount) cần input")
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(needsInputCount) session cần input")
            }
            Button {
                model.bridge.showingSettings = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "iphone").font(.system(size: 11))
                    Text("Điện thoại")
                    Circle().fill(model.bridge.isRunning ? Token.statusRunning : Token.statusExited).frame(width: 6, height: 6)
                }
            }
            .buttonStyle(SecondaryButtonStyle())
            .accessibilityLabel(model.bridge.isRunning ? "Điện thoại: đang cho kết nối" : "Điện thoại: đang tắt")
            .help("Kết nối điện thoại")
            search
        }
        .padding(.horizontal, Space.md)
        .frame(height: Metric.topBarHeight)
        .background(Token.surface)
        .overlay(alignment: .bottom) { Divider1() }
        .background {
            // ⌘K focuses the search field from anywhere.
            Button("Tìm session") { searchFocused.wrappedValue = true }
                .keyboardShortcut("k", modifiers: .command)
                .opacity(0).frame(width: 0, height: 0).accessibilityHidden(true)
        }
    }

    private var search: some View {
        HStack(spacing: Space.sm) {
            Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(Token.muted).accessibilityHidden(true)
            TextField("Tìm session", text: $model.searchText)
                .textFieldStyle(.plain)
                .font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
                .focused(searchFocused)
                .onSubmit { model.selectFirstSearchMatch() }
                .onExitCommand { model.searchText = ""; searchFocused.wrappedValue = false }
                .accessibilityLabel("Tìm session")
            if model.searchText.isEmpty {
                Text("⌘K").font(TypeToken.codeSm).foregroundStyle(Token.muted).accessibilityHidden(true)
            } else {
                Button { model.searchText = "" } label: { Image(systemName: "xmark.circle.fill").font(.system(size: 11)) }
                    .buttonStyle(.plain).foregroundStyle(Token.muted)
                    .accessibilityLabel("Xoá tìm kiếm")
            }
        }
        .padding(.horizontal, Space.sm)
        .frame(width: 280, height: 28)
        .background(Token.neutral, in: RoundedRectangle(cornerRadius: Radius.md))
        .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(searchFocused.wrappedValue ? Token.primary : Token.border,
                                                                   lineWidth: searchFocused.wrappedValue ? 2 : 1))
    }
}
