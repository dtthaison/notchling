import ConsoleCore
import SwiftUI

/// Tab bar plus the content of each tab: Flow, Diagram, Entity and Chart render in the shared viz bundle
/// (WKWebView); Phone streams a device window; Web shows the session's dev server.
struct VisualizePanel: View {
    @Bindable var model: ConsoleModel

    var body: some View {
        VStack(spacing: 0) {
            if let session = model.selectedSession {
                tabBar(for: session)
                Divider1()
                content(for: model.visualizeTab(for: session), session: session)
            } else {
                PanelHeader(title: "Visualize") { EmptyView() }
                Text("Chọn một session để xem visualize.")
                    .font(TypeToken.bodyMd).foregroundStyle(Token.muted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Token.surface)
    }

    @ViewBuilder
    private func content(for tab: VisualizeTab, session: Session) -> some View {
        switch tab {
        case .phone: PhoneTab(model: model, session: session)
        case .web: WebTab(model: model, session: session)
        case .flow: VizContent(model: model, session: session, kind: .flow)
        case .diagram: VizContent(model: model, session: session, kind: .diagram)
        case .entity: VizContent(model: model, session: session, kind: .entity)
        case .chart: VizContent(model: model, session: session, kind: .chart)
        }
    }

    private func tabBar(for session: Session) -> some View {
        let selected = model.visualizeTab(for: session)
        let suggested = model.suggestedTab(for: session)
        return HStack(spacing: Space.xs) {
            Text("Visualize").panelTitleStyle().padding(.trailing, Space.sm).accessibilityAddTraits(.isHeader)
            ForEach(VisualizeTab.allCases) { tab in
                Button {
                    model.chooseVisualizeTab(tab, for: session)
                } label: {
                    HStack(spacing: 4) {
                        Text(tab.title)
                        if tab == suggested {
                            Circle().fill(Token.muted).frame(width: 5, height: 5)
                        }
                    }
                    .font(TypeToken.labelSm)
                    .foregroundStyle(tab == selected ? Token.primary : Token.muted)
                    .padding(.horizontal, Space.sm)
                    .frame(height: 24)
                    .background(tab == selected ? Token.surfaceRaised : Color.clear, in: RoundedRectangle(cornerRadius: Radius.sm))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .animation(Motion.hover, value: selected)
                .accessibilityLabel(tab == suggested ? "\(tab.title), gợi ý cho session này" : tab.title)
                .accessibilityAddTraits(tab == selected ? [.isButton, .isSelected] : .isButton)
            }
            Spacer()
        }
        .padding(.horizontal, Space.md)
        .frame(height: Metric.panelHeaderHeight)
        .background(Token.surface)
    }
}
