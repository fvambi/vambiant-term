import Foundation
import Testing
@testable import VambiantTerm

struct AgentTests {
    private func answer(_ text: String, cost: Double? = 0.0041, tokens: (Int, Int) = (1234, 340)) -> AgentAnswer {
        AgentAnswer(
            text: text, profile: "claude-strong", model: "claude-sonnet-5", inputTokens: tokens.0,
            outputTokens: tokens.1, costUSD: cost, redactions: 2, truncated: false, seconds: 3.24
        )
    }

    @Test func stagesFencedShellBlocksAndDollarLines() {
        let text = """
        Run this:
        ```sh
        # comment lines are skipped
        $ git status
        git diff --stat
        ```
        Then `$ echo done` inline is prose, but a line
        $ echo done
        counts once even when repeated:
        ```
        echo done
        ```
        ```python
        print("not a command")
        ```
        """
        #expect(AgentAnswer.commands(in: text) == ["git status", "git diff --stat", "echo done"])
    }

    @Test func footerNamesModelTokensCostRedactionsAndTime() {
        #expect(answer("x").footer == "claude-sonnet-5 · 1.2k in / 340 out · ≈$0.0041 · 2 redactions · 3.2s")
        var cut = answer("x", cost: nil, tokens: (12, 5))
        cut.truncated = true
        cut.redactions = 0
        #expect(cut.footer == "claude-sonnet-5 · 12 in / 5 out · 3.2s · cut off at max tokens")
    }

    @Test func conversationHoldsOneRequestAtATime() {
        var c = AgentConversation()
        let first = c.ask("why", profile: "route ask")
        #expect(first)
        #expect(c.isThinking)
        let second = c.ask("again", profile: "route ask")
        #expect(!second)
        #expect(c.history.isEmpty, "the in-flight prompt is not history")
        c.answer(answer("Run pwd."))
        #expect(!c.isThinking)
        #expect(c.history == [
            AgentHistoryTurn(role: "user", text: "why"), AgentHistoryTurn(role: "assistant", text: "Run pwd."),
        ])
        let third = c.ask("and then", profile: "route ask")
        #expect(third)
        c.fail("redaction failed: timeout")
        #expect(c.turns.last == .failure("redaction failed: timeout"))
        #expect(c.history.count == 2, "failures are shown but not resent")
        #expect(c.totalCostUSD == 0.0041)
        #expect(c.totalTokens == (1234, 340))
    }

    @Test func deltasStreamIntoTheThinkingRow() {
        var c = AgentConversation()
        c.ask("why", profile: "route ask")
        c.request = "r1"
        c.append(delta: "Run ")
        c.append(delta: "pwd.")
        #expect(c.turns.last == .streaming(partial: "Run pwd.", since: c.since ?? .distantPast))
        #expect(c.isThinking)
        #expect(c.history.isEmpty)
        c.answer(answer("Run pwd."))
        #expect(c.request == nil, "the request ends with the answer")
        c.append(delta: "late")
        #expect(c.turns.last == .answer(answer("Run pwd.")), "deltas after the end are ignored")
    }

    @Test func replyDecodesTheDaemonsShape() throws {
        let json = """
        {"text":"Run pwd.","profile":"mock","model":"test-model",
         "usage":{"input_tokens":30,"output_tokens":6,"cache_read_tokens":0,"cache_write_tokens":0},
         "cost_usd_estimate":0.00012,"redactions":1,"stop":"max_tokens"}
        """
        let reply = try JSONDecoder().decode(AgentAskReply.self, from: Data(json.utf8))
        let a = reply.answer(seconds: 1)
        #expect(a.truncated)
        #expect(a.inputTokens == 30 && a.outputTokens == 6 && a.redactions == 1)
        let other = try JSONDecoder().decode(
            AgentAskReply.self, from: Data(json.replacingOccurrences(of: "\"max_tokens\"", with: "{\"other\":\"x\"}").utf8)
        )
        #expect(!other.answer(seconds: 1).truncated)
    }

    @Test func panelTitleSumsTheConversation() {
        var c = AgentConversation()
        c.ask("a", profile: "route ask")
        #expect(AgentPanel.title(for: c) == "Agent · route ask")
        c.answer(answer("x"))
        c.ask("b", profile: "route ask")
        c.answer(answer("y", cost: 0.02, tokens: (2000, 100)))
        #expect(AgentPanel.title(for: c) == "Agent · claude-strong · ≈$0.02 · 3.2k in / 440 out")
    }

    @Test func keymapKnowsTheAgentActions() {
        #expect(ShellAction.from(id: "ai.ask", label: "", milestone: "") == .askAgent)
        #expect(ShellAction.from(id: "ai.explain_last_failure", label: "", milestone: "") == .explainLastFailure)
    }
}
