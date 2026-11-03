import ConsoleCore
import SwiftUI

struct RootView: View {
    @Bindable var model: ConsoleModel
    @AppStorage("split.columnRatio") private var columnRatio = 0.44
    @AppStorage("split.chatRatio") private var chatRatio = 0.60
    @FocusState private var searchFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            TopBar(model: model, searchFocused: $searchFocused)
            ResizableSplit(axis: .horizontal, ratio: $columnRatio, minFirst: 480, minSecond: 360) {
                ResizableSplit(axis: .vertical, ratio: $chatRatio, minFirst: 220, minSecond: 160) {
                    ChatPanel(model: model)
                } second: {
                    SessionListPanel(model: model)
                }
            } second: {
                VisualizePanel(model: model)
            }
        }
        .background(Token.neutral)
        .foregroundStyle(Token.onSurface)
        .preferredColorScheme(.dark)
        .sheet(isPresented: $model.showingNewSession) {
            NewSessionSheet(model: model).panelEntrance()
        }
        // One sheet for both: SwiftUI cannot stack two, and a pairing request must show over the settings.
        .sheet(isPresented: Binding(
            get: { model.bridge.showingSettings || model.bridge.approval != nil },
            set: { shown in
                guard !shown else { return }
                if model.bridge.approval != nil { model.bridge.resolveApproval(false) }
                model.bridge.showingSettings = false
            })) {
            if let approval = model.bridge.approval {
                ApprovalSheet(approval: approval, model: model)
            } else {
                BridgeSettingsView(model: model)
            }
        }
        .modifier(TakeoverDialogs(model: model))
        .task { model.bridge.attach(model: model) }
        .navigationTitle("Agent Console")
        .task { model.viz.start() }
        .onChange(of: model.store.selection) { _, _ in model.syncViz() }
        .onChange(of: model.sessions) { _, _ in model.syncViz() }
        .onChange(of: model.sessions) { _, sessions in
            let agents = sessions.reduce(0) { $0 + $1.subagents.count }
            ConsoleLog.logger.notice("snapshot sessions=\(sessions.count) subagents=\(agents)")
        }
    }
}
