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

    @Test func budgetWarningsAtEightyAndHundredPercent() {
        func b(_ used: Double, limit: Double = 5) -> AgentBudget {
            AgentBudget(dailyUsed: used, dailyLimit: limit, monthlyUsed: 0, monthlyLimit: 60, hardStop: true)
        }
        #expect(b(1).warning == nil)
        #expect(b(4.1).warning == "AI budget at 82%: $4.10 of $5.00 today")
        #expect(b(5.2).warning?.hasPrefix("AI budget reached: $5.20 of $5.00 today") == true)
        #expect(b(5.2).warning?.hasSuffix("further cloud requests are refused") == true)
        var a = answer("x")
        a.budget = (used: 0.5, limit: 5)
        #expect(a.footer.hasSuffix("· $0.50 of $5.00 today"))
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
        #expect(ShellAction.from(id: "ai.show_last_payload", label: "", milestone: "") == .showLastPayload)
        #expect(ShellAction.from(id: "history.search", label: "", milestone: "") == .historySearch)
    }
}

struct AgentToolTests {
    private func call(_ id: String, _ command: String) -> AgentToolCall {
        AgentToolCall(id: id, command: command, why: "to see", verdictClass: "destructive", floor: true, decision: "ask", applied: false)
    }

    @Test func toolCallsSplitTheStreamedTextAndKeepTheirPlace() {
        var c = AgentConversation()
        c.ask("clean up", profile: "route agent")
        c.request = "r"
        c.append(delta: "Let me check. ")
        c.toolRequest(call("t1", "rm -rf ./dist"))
        #expect(c.turns.count == 4, "\(c.turns)")
        #expect(c.turns[1] == .text("Let me check. "))
        if case let .tool(t) = c.turns[2] {
            #expect(t.statusLine.contains("waiting for your approval"))
            #expect(t.label == "destructive · never auto")
        } else {
            Issue.record("expected the tool turn")
        }
        #expect(c.isThinking)
        c.toolResult(id: "t1", status: .done(exit: 0), command: "rm -rf ./dist", output: "")
        c.append(delta: "Done.")
        c.answer(AgentAnswer(
            text: "Let me check. \n\nDone.", profile: "p", model: "m", inputTokens: 1, outputTokens: 1, costUSD: nil,
            redactions: 0, truncated: false, seconds: 1
        ))
        if case let .answer(a) = c.turns.last {
            #expect(a.text == "Done.", "only the last segment is shown; the rest is in place")
            #expect(a.transcript == "Let me check. \n\nDone.")
        } else {
            Issue.record("expected the answer")
        }
        #expect(c.history.last?.text == "Let me check. \n\nDone.", "history carries the whole run")
        #expect(!c.turns.contains {
            if case .thought = $0 {
                true
            } else {
                false
            }
        }, "a sub-second answer gets no thought row")
        if case let .tool(t) = c.turns[2] {
            #expect(t.statusLine == "exit 0")
        }
    }

    @Test func deniedAndFailedCallsSayWhy() {
        var c = AgentConversation()
        c.ask("x", profile: "route agent")
        c.toolRequest(call("t1", "sudo rm -rf /"))
        c.toolResult(id: "t1", status: .denied(reason: "not today"), command: nil, output: nil)
        c.toolResult(id: "nope", status: .done(exit: 0), command: nil, output: nil)
        if case let .tool(t) = c.turns[1] {
            #expect(t.statusLine == "denied: not today")
        } else {
            Issue.record("expected the tool turn at 1, got \(c.turns)")
        }
        var busy = call("t2", "sleep 1")
        busy.status = .failed(message: "the session is busy")
        #expect(busy.statusLine == "not run: the session is busy")
        var auto = call("t3", "ls")
        auto.applied = true
        auto.verdictClass = "benign"
        #expect(auto.statusLine == "decided by policy" && auto.label == nil)
    }
}
