import AppKit
import SwiftUI

/// Two panes split by a draggable 1 px divider (6 px hit area). The ratio is the first pane's share
/// and is stored by the caller (AppStorage), so it survives relaunch.
struct ResizableSplit<First: View, Second: View>: View {
    enum Axis { case horizontal, vertical }

    let axis: Axis
    @Binding var ratio: Double
    var minFirst: CGFloat = 200
    var minSecond: CGFloat = 200
    @ViewBuilder var first: () -> First
    @ViewBuilder var second: () -> Second

    @State private var dragStartRatio: Double?

    var body: some View {
        GeometryReader { proxy in
            let total = axis == .horizontal ? proxy.size.width : proxy.size.height
            let firstSize = clamp(total * ratio, total: total)
            Group {
                if axis == .horizontal {
                    HStack(spacing: 0) {
                        first().frame(width: firstSize)
                        divider(total: total)
                        second().frame(maxWidth: .infinity)
                    }
                } else {
                    VStack(spacing: 0) {
                        first().frame(height: firstSize)
                        divider(total: total)
                        second().frame(maxHeight: .infinity)
                    }
                }
            }
        }
    }

    private func clamp(_ size: CGFloat, total: CGFloat) -> CGFloat {
        guard total > minFirst + minSecond else { return max(0, total / 2) }
        return min(max(size, minFirst), total - minSecond)
    }

    private func divider(total: CGFloat) -> some View {
        Token.border
            .frame(width: axis == .horizontal ? 1 : nil, height: axis == .vertical ? 1 : nil)
            .overlay {
                Color.clear
                    .frame(width: axis == .horizontal ? 7 : nil, height: axis == .vertical ? 7 : nil)
                    .contentShape(Rectangle())
                    .onHover { inside in
                        if inside { (axis == .horizontal ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown).push() } else { NSCursor.pop() }
                    }
                    .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                        .onChanged { value in
                            let start = dragStartRatio ?? ratio
                            dragStartRatio = start
                            let delta = axis == .horizontal ? value.translation.width : value.translation.height
                            ratio = Double(clamp(total * start + delta, total: total) / total)
                        }
                        .onEnded { _ in dragStartRatio = nil })
            }
            .accessibilityElement()
            .accessibilityLabel(axis == .horizontal ? "Chia cột" : "Chia chat và danh sách")
            .accessibilityAdjustableAction { direction in
                let step = 0.04
                switch direction {
                case .increment: ratio = min(0.85, ratio + step)
                case .decrement: ratio = max(0.15, ratio - step)
                @unknown default: break
                }
            }
    }
}
