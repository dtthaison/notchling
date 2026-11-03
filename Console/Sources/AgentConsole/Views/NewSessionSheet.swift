import AppKit
import ConsoleCore
import SwiftUI

/// Pick a folder and a model, then Console starts `claude` in a pty it owns. The folder-trust prompt
/// is the user's to answer in the Terminal view.
struct NewSessionSheet: View {
    @Bindable var model: ConsoleModel
    @State private var folder: URL?
    @State private var chosenModel: NewSessionModel = .defaultModel

    var body: some View {
        VStack(alignment: .leading, spacing: Space.lg) {
            Text("Session mới").font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
                .accessibilityAddTraits(.isHeader)

            VStack(alignment: .leading, spacing: Space.sm) {
                Text("Thư mục").font(TypeToken.labelSm).foregroundStyle(Token.muted)
                HStack(spacing: Space.sm) {
                    Text(folder?.path ?? "Chưa chọn thư mục")
                        .font(TypeToken.codeSm).foregroundStyle(folder == nil ? Token.muted : Token.onSurface)
                        .lineLimit(1).truncationMode(.head)
                        .padding(.horizontal, Space.sm).frame(maxWidth: .infinity, minHeight: Metric.inputHeight, alignment: .leading)
                        .background(Token.neutral, in: RoundedRectangle(cornerRadius: Radius.md))
                        .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
                    Button("Chọn…", action: chooseFolder).buttonStyle(SecondaryButtonStyle())
                        .accessibilityLabel("Chọn thư mục")
                }
            }

            VStack(alignment: .leading, spacing: Space.sm) {
                Text("Model").font(TypeToken.labelSm).foregroundStyle(Token.muted)
                HStack(spacing: Space.xs) {
                    ForEach(NewSessionModel.allCases) { option in
                        Button { chosenModel = option } label: {
                            Text(option.title)
                                .font(TypeToken.labelSm)
                                .foregroundStyle(option == chosenModel ? Token.primary : Token.muted)
                                .padding(.horizontal, Space.md).frame(height: 24)
                                .background(option == chosenModel ? Token.surfaceRaised : Color.clear,
                                            in: RoundedRectangle(cornerRadius: Radius.sm))
                                .overlay(RoundedRectangle(cornerRadius: Radius.sm)
                                    .stroke(option == chosenModel ? Token.border : Color.clear, lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Model \(option.title)")
                        .accessibilityAddTraits(option == chosenModel ? [.isButton, .isSelected] : .isButton)
                    }
                }
            }

            HStack(alignment: .top, spacing: Space.sm) {
                Image(systemName: "info.circle").font(.system(size: 11)).accessibilityHidden(true)
                Text("Với thư mục mới, Claude sẽ hỏi có tin cậy không. Câu đó hiện trong Terminal của session: bạn tự trả lời.")
                    .font(TypeToken.bodySm)
            }
            .foregroundStyle(Token.muted)

            if let error = model.newSessionError {
                Text(error).font(TypeToken.bodySm).foregroundStyle(Token.statusError)
            }

            HStack(spacing: Space.sm) {
                Spacer()
                Button("Huỷ") { model.showingNewSession = false }
                    .buttonStyle(SecondaryButtonStyle()).keyboardShortcut(.cancelAction)
                Button("Mở session") {
                    if let folder { model.startSession(folder: folder, model: chosenModel) }
                }
                .buttonStyle(PrimaryButtonStyle())
                .disabled(folder == nil)
                .keyboardShortcut(.defaultAction)
                .accessibilityLabel("Mở session")
            }
        }
        .padding(Space.xl)
        .frame(width: 480)
        .background(Token.surface)
        .preferredColorScheme(.dark)
        .onAppear { model.newSessionError = nil }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Chọn"
        panel.message = "Thư mục để chạy Claude Code"
        if panel.runModal() == .OK { folder = panel.url }
    }
}
