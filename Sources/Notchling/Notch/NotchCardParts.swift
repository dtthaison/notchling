import SwiftUI

/// Carries a card's laid-out height up to the model, so the panel fits it exactly.
struct CardHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

extension View {
    func reportsCardHeight(to model: NotchModel) -> some View {
        fixedSize(horizontal: false, vertical: true)
            .background(GeometryReader { Color.clear.preference(key: CardHeightKey.self, value: $0.size.height) })
            .onPreferenceChange(CardHeightKey.self) { h in
                if h > 0, abs((model.cardHeight ?? 0) - h) > 0.5 { model.cardHeight = h }
            }
    }
}

extension View {
    /// Fades the cut-off edge of a scrolling card so it reads as "more below".
    func moreBelow(_ show: Bool) -> some View {
        overlay(alignment: .bottom) {
            if show {
                LinearGradient(colors: [.black.opacity(0), .black], startPoint: .top, endPoint: .bottom)
                    .frame(height: 14)
                    .allowsHitTesting(false)
            }
        }
    }
}

/// Natural height of a `ScrollingBody`'s content; its own key, so it doesn't mix with `CardHeightKey`.
struct BodyHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// Content at its full height up to `limit`, scrolling beyond that, so a long question keeps its
/// header and answer box in view. Back at the top whenever `resetKey` changes.
struct ScrollingBody<Content: View>: View {
    let limit: CGFloat
    let resetKey: Int
    @ViewBuilder var content: Content
    @State private var height: CGFloat = 0

    var body: some View {
        let overflows = height > limit
        ScrollView(.vertical) {
            content.fixedSize(horizontal: false, vertical: true)
                .background(GeometryReader { Color.clear.preference(key: BodyHeightKey.self, value: $0.size.height) })
                .padding(.bottom, overflows ? 14 : 0)
        }
        .onPreferenceChange(BodyHeightKey.self) { height = $0 }
        .scrollIndicators(overflows ? .visible : .hidden)
        .id(resetKey)
        .frame(height: height > 0 ? min(height, limit) : nil)
        .moreBelow(overflows)
    }
}
