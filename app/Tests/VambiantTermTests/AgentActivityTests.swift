import Foundation
import Testing
@testable import VambiantTerm

struct AgentActivityTests {
    private func event(_ json: String) -> JSONValue {
        (try? JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))) ?? .null
    }

    @Test func foldsToolCallsTasksAndThinking() {
        var a = AgentActivity()
        #expect(a.isEmpty && a.headline == nil)
        a.apply(event(#"{"type":"tool_call_start","id":"t1","name":"Bash","input":{"command":"cargo test"}}"#))
        #expect(a.headline == "⚙ Bash · cargo test")
        a.apply(event(#"{"type":"tool_call_end","id":"t1","ok":true,"output":"ok","duration_ms":1200}"#))
        #expect(a.headline == "✔ Bash · cargo test · 1.2s")
        let todos = ##"[{"content":"Write the parser","status":"in_progress"},{"content":"Add tests","status":"pending"},"## +
            ##"{"content":"Read spec","status":"completed"}]"##
        a.apply(event(##"{"type":"tool_call_start","id":"t2","name":"TodoWrite","input":{"todos":"## + todos + "}}"))
        #expect(a.tasks.count == 3)
        #expect(a.taskLine == "1/3 tasks · ● Write the parser")
        #expect(a.headline == "⚙ TodoWrite · 3 tasks")
        a.apply(event(#"{"type":"tool_call_end","id":"t2","ok":true,"output":null,"duration_ms":10}"#))
        a.apply(event(#"{"type":"thinking","summary":null}"#))
        #expect(a.headline == "◐ thinking…")
        a.apply(event(#"{"type":"tool_call_start","id":"t3","name":"Edit","input":{"file_path":"/a/b/src/main.rs"}}"#))
        #expect(a.headline == "⚙ Edit · …/src/main.rs")
        a.apply(event(#"{"type":"tool_call_end","id":"t3","ok":false,"output":"nope","duration_ms":5}"#))
        #expect(a.headline?.hasPrefix("✕ Edit") == true)
        a.apply(event(#"{"type":"file_changed","path":"/a/b/src/main.rs","diff":null}"#))
        #expect(a.files == ["/a/b/src/main.rs"])
        a.apply(event(#"{"type":"assistant_text","text":"Done.\nMore.","streaming":false}"#))
        #expect(a.headline == "💬 Done.")
        a.apply(event(#"{"type":"state_changed","from":"idle","to":"thinking"}"#))
        #expect(a.thinking && a.headline == "◐ thinking…")
        #expect(a.log.count == 9, "\(a.log)")
        #expect(a.log.allSatisfy { $0.count > 10 })
    }
}
