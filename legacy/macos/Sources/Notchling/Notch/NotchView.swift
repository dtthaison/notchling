import SwiftUI

/// Black shape that continues the camera housing: small outward flares where it meets the
/// top of the screen, rounded corners at the bottom.
struct NotchShape: Shape {
    var radius: CGFloat
    var flare: CGFloat = NotchModel.flare

    var animatableData: CGFloat {
        get { radius }
        set { radius = newValue }
    }

    func path(in r: CGRect) -> Path {
        let f = flare
        let radius = min(self.radius, (r.height - f) / 2, (r.width - 2 * f) / 2)
        var p = Path()
        p.move(to: CGPoint(x: r.minX, y: r.minY))
        p.addQuadCurve(to: CGPoint(x: r.minX + f, y: r.minY + f), control: CGPoint(x: r.minX + f, y: r.minY))
        p.addLine(to: CGPoint(x: r.minX + f, y: r.maxY - radius))
        p.addQuadCurve(to: CGPoint(x: r.minX + f + radius, y: r.maxY), control: CGPoint(x: r.minX + f, y: r.maxY))
        p.addLine(to: CGPoint(x: r.maxX - f - radius, y: r.maxY))
        p.addQuadCurve(to: CGPoint(x: r.maxX - f, y: r.maxY - radius), control: CGPoint(x: r.maxX - f, y: r.maxY))
        p.addLine(to: CGPoint(x: r.maxX - f, y: r.minY + f))
        p.addQuadCurve(to: CGPoint(x: r.maxX, y: r.minY), control: CGPoint(x: r.maxX - f, y: r.minY))
        p.closeSubpath()
        return p
    }
}

extension Color {
    static let claude = Color(red: 0.85, green: 0.47, blue: 0.34)
    static let asks = Color(red: 0.36, green: 0.74, blue: 0.86)
    static let optionFill = Color(red: 0.06, green: 0.12, blue: 0.14)
    static let optionBadge = Color(red: 0.13, green: 0.27, blue: 0.30)
}

extension AgentGroup {
    /// Short enough for three tabs to fit under the camera.
    var shortTitle: String {
        switch self {
        case .needsInput: return "Input"
        case .working: return "Working"
        case .completed: return "Done"
        }
    }

    var color: Color {
        switch self {
        case .needsInput: return .orange
        case .working: return .blue
        case .completed: return .green
        }
    }
}

extension Agent {
    var color: Color {
        switch (group, label) {
        case (.needsInput, _): return .orange
        case (.working, _): return .blue
        case (_, "Done"): return .green
        case (_, "Failed"), (_, "Failure"), (_, "Error"): return .red
        default: return .gray
        }
    }

    /// Fits a narrow row: "bg", "VS Code", "-p".
    var shortHost: String {
        switch host {
        case "Background": return "bg"
        case "claude -p": return "-p"
        default: return host
        }
    }

    var ago: String {
        let s = max(0, Int(Date().timeIntervalSince(since)))
        if s < 60 { return "now" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86400 { return "\(s / 3600)h" }
        return "\(s / 86400)d"
    }
}

func oneLine(_ s: String) -> String {
    s.replacingOccurrences(of: "\n", with: " ").trimmingCharacters(in: .whitespaces)
}

struct NotchView: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        let size = model.size(for: model.state)
        VStack(spacing: 0) {
            topBar.frame(height: model.notchSize.height).part("Notch Bar", .bottom)
            Group {
                switch model.state {
                case .collapsed: EmptyView()
                case .peek(let id): if let agent = model.agent(id) { PeekView(model: model, agent: agent).part("Peek", .bottom, inside: true) }
                case .list: AgentListView(model: model).part("Notch List", .bottom, inside: true)
                case .detail(let id): if let agent = model.agent(id) { DetailView(model: model, agent: agent).part("Notch Detail", .bottom, inside: true) }
                }
            }
            // Content grows out of the notch as the shape opens, and fades quickly as it closes.
            .transition(.asymmetric(
                insertion: .scale(scale: 0.6, anchor: .top).combined(with: .opacity)
                    .animation(.spring(response: 0.36, dampingFraction: 0.9).delay(0.05)),
                removal: .opacity.animation(.easeIn(duration: 0.12))))
        }
        .padding(.horizontal, NotchModel.flare)
        .frame(width: size.width, height: size.height, alignment: .top)
        .background(Color.black)
        .clipShape(NotchShape(radius: model.state == .collapsed ? 10 : 24))
        // The pet lives outside the black, so it sits over the shape rather than inside it.
        .overlay(alignment: .topLeading) {
            if model.showPet {
                PetView(model: model, shape: size)
            }
        }
        // The resting strip grows a lane for the pet's walks, and folds it away again.
        .animation(.spring(response: 0.4, dampingFraction: 0.9), value: size)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .preferredColorScheme(.dark)
    }

    /// The number of active sessions to the right of the camera. When the list opens it slides in
    /// under the camera with the pet, and the tabs grow out of that spot.
    private var topBar: some View {
        let active = model.needsCount + model.workingCount
        let gathered = model.state == .list
        // From each wing to the middle of the tab row just under the camera.
        let across = model.notchSize.width / 2 + NotchModel.wingWidth / 2 - 9
        let down = model.notchSize.height / 2 + TabBar.height / 2

        return HStack(spacing: 0) {
            Color.clear.frame(width: NotchModel.wingWidth)
            Spacer(minLength: model.notchSize.width)
            Text(active > 0 ? "\(active)" : "")
                .font(.system(size: 12.5, weight: .semibold, design: .rounded))
                .foregroundColor(model.needsCount > 0 ? .orange : .white)
                .frame(width: NotchModel.wingWidth)
                .offset(x: gathered ? -across : 0, y: gathered ? down : 0)
                .animation(.spring(response: 0.3, dampingFraction: 0.85), value: gathered)
        }
        .frame(width: model.notchSize.width + 2 * NotchModel.wingWidth)
        // Fade only once they've met in the middle; reappear straight away on the way back.
        .opacity(gathered ? 0 : 1)
        .animation(gathered ? .easeOut(duration: 0.12).delay(0.24) : .easeIn(duration: 0.08), value: gathered)
    }
}
