// One pane: the grid on top, Warp's input area at the bottom (ADR-0011),
// and the Agent Mode conversation between them while it is open. In
// classic mode the input area is hidden and the grid fills the pane.

import AppKit

@MainActor
final class PaneView: NSView {
    let grid: MetalGridView
    let input = InputAreaView()
    /// The approval card for this session's waiting agent, above the rest.
    let approval = ApprovalCard()
    let agent = AgentPanel()
    /// Share of the pane the conversation takes while open.
    static let agentShare: CGFloat = 0.45
    var warpMode = true {
        didSet {
            input.isHidden = !warpMode
            needsLayout = true
        }
    }

    init(grid: MetalGridView) {
        self.grid = grid
        super.init(frame: .zero)
        addSubview(grid)
        addSubview(approval)
        addSubview(agent)
        addSubview(input)
        agent.isHidden = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    override var isFlipped: Bool {
        true
    }

    override func layout() {
        super.layout()
        let scale = window?.backingScaleFactor ?? 2
        let lineHeight = grid.renderer.cellSize.height / scale
        let inputHeight = warpMode ? InputAreaView.height(lines: input.editor.lineCount, lineHeight: lineHeight) : 0
        input.relayout(lineHeight: lineHeight)
        input.frame = CGRect(x: 0, y: bounds.height - inputHeight, width: bounds.width, height: inputHeight)
        let agentHeight = agent.isHidden ? 0 : ((bounds.height - inputHeight) * Self.agentShare).rounded()
        agent.frame = CGRect(x: 0, y: bounds.height - inputHeight - agentHeight, width: bounds.width, height: agentHeight)
        let cardHeight = approval.isHidden ? 0 : ApprovalCard.height
        approval.frame = CGRect(
            x: 8, y: bounds.height - inputHeight - agentHeight - cardHeight, width: bounds.width - 16, height: cardHeight - 6
        )
        grid.frame = CGRect(x: 0, y: 0, width: bounds.width, height: bounds.height - inputHeight - agentHeight - cardHeight)
    }
}
