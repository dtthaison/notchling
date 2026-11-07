import Foundation

/// Token use from the other tools' session files, for `CostTracker`. Each parser is pure (one decoded
/// line in, an event out) so it can be tested without a tracker.

/// Codex's `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Three kinds of line matter:
/// - `session_meta`: `creator_user_id` and `creator_account_id` name the login (the id `CodexStore` uses).
/// - `turn_context`: the `model` and `cwd` the following requests run with.
/// - `event_msg` / `token_count`: `info.last_token_usage` is what the latest request used and
///   `info.total_token_usage` the running sum. Only `last_token_usage` is counted; the running sum is
///   used to drop an event Codex wrote twice (same total as the one before it).
/// `input_tokens` includes `cached_input_tokens`, and `output_tokens` includes the reasoning tokens.
enum CodexUsage {
    struct Tokens: Equatable {
        var total: Double
        var input: Double, cached: Double, output: Double
        var date: Date
        var ordinal: Double?
    }

    enum Event: Equatable {
        case session(account: String?)
        case turn(model: String?, folder: String?)
        case tokens(Tokens)
    }

    static func parse(_ object: [String: Any], date: (String) -> Date?) -> Event? {
        guard let payload = object["payload"] as? [String: Any] else { return nil }
        switch object["type"] as? String {
        case "session_meta":
            guard let user = payload["creator_user_id"] as? String, let account = payload["creator_account_id"] as? String else {
                return .session(account: nil)
            }
            return .session(account: "\(user):\(account)")
        case "turn_context":
            return .turn(model: payload["model"] as? String, folder: payload["cwd"] as? String)
        case "event_msg":
            guard payload["type"] as? String == "token_count",
                  let info = payload["info"] as? [String: Any],
                  let last = info["last_token_usage"] as? [String: Any],
                  let stamp = object["timestamp"] as? String, let when = date(stamp) else { return nil }
            func n(_ key: String, _ from: [String: Any]) -> Double { (from[key] as? NSNumber)?.doubleValue ?? 0 }
            let total = (info["total_token_usage"] as? [String: Any]).map { n("total_tokens", $0) } ?? -1
            return .tokens(Tokens(total: total, input: n("input_tokens", last), cached: n("cached_input_tokens", last),
                                  output: n("output_tokens", last), date: when,
                                  ordinal: (object["ordinal"] as? NSNumber)?.doubleValue))
        default:
            return nil
        }
    }

    /// Input (not cached), output, cache read, cache write. OpenAI has no cache-write charge.
    static func counts(_ t: Tokens) -> [Double] { [max(0, t.input - t.cached), t.output, t.cached, 0] }
}

/// Antigravity CLI's `brain/<conversation>/.system_generated/logs/transcript.jsonl`. A `PLANNER_RESPONSE`
/// step carries `input_tokens` (not counting the cache), `cache_read_tokens` and `output_tokens` for its
/// request, without a model. The model is the last "Model Selection" setting change in a `USER_INPUT`
/// step ("changed setting `Model Selection` from None to Gemini 4 Argon (Medium)").
enum AntigravityUsage {
    struct Tokens: Equatable {
        var input: Double, cacheRead: Double, output: Double
        var date: Date
        var step: Double
    }

    enum Event: Equatable {
        case model(String)
        case tokens(Tokens)
    }

    static func parse(_ object: [String: Any], date: (String) -> Date?) -> Event? {
        switch object["type"] as? String {
        case "USER_INPUT":
            guard let content = object["content"] as? String, let model = model(in: content) else { return nil }
            return .model(model)
        case "PLANNER_RESPONSE":
            guard let input = object["input_tokens"] as? NSNumber, let step = object["step_index"] as? NSNumber,
                  let stamp = object["created_at"] as? String, let when = date(stamp) else { return nil }
            func n(_ key: String) -> Double { (object[key] as? NSNumber)?.doubleValue ?? 0 }
            return .tokens(Tokens(input: input.doubleValue, cacheRead: n("cache_read_tokens"), output: n("output_tokens"),
                                  date: when, step: step.doubleValue))
        default:
            return nil
        }
    }

    /// "Gemini 4 Argon" from "...changed setting `Model Selection` from None to Gemini 4 Argon (Medium). No need...".
    static func model(in content: String) -> String? {
        let pattern = #"`Model Selection` from .+? to (.+?)(?: \([A-Za-z ]+\))?\. No need"#
        guard content.contains("`Model Selection`"),
              let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: content, range: NSRange(content.startIndex..., in: content)),
              let range = Range(match.range(at: 1), in: content) else { return nil }
        return String(content[range])
    }

    static func counts(_ t: Tokens) -> [Double] { [t.input, t.output, t.cacheRead, 0] }
}

/// Dollars per million tokens for models outside Claude, at OpenAI's API list prices: input, cached input,
/// output. Source: openai.com/api/pricing, as of 2026-11-06. A model not listed (the gpt-6.x family, Gemini)
/// has no entry because no list price is published for it that Notchling knows: its tokens are still counted
/// and its cost is 0. Add a line here when a price is known. Ids match whole, minus a `-YYYY-MM-DD` suffix, so
/// "gpt-5.6-sol" can't pick up "gpt-5"'s price.
enum OtherPrices {
    static let table: [String: (input: Double, cached: Double, output: Double)] = [
        "gpt-5": (1.25, 0.125, 10), "gpt-5-codex": (1.25, 0.125, 10),
        "gpt-5.1": (1.25, 0.125, 10), "gpt-5.1-codex": (1.25, 0.125, 10), "gpt-5.1-codex-max": (1.25, 0.125, 10),
        "gpt-5-mini": (0.25, 0.025, 2), "gpt-5.1-codex-mini": (0.25, 0.025, 2),
        "gpt-5-nano": (0.05, 0.005, 0.40),
        "gpt-4.1": (2, 0.50, 8), "o3": (2, 0.50, 8), "o4-mini": (1.10, 0.275, 4.40),
    ]

    /// Dollars for one request. `counts` is input (not cached), output, cache read.
    static func cost(model: String, counts: [Double]) -> Double {
        let id = model.lowercased().replacingOccurrences(of: #"-\d{4}-\d{2}-\d{2}$"#, with: "", options: .regularExpression)
        guard let price = table[id], counts.count >= 3 else { return 0 }
        return (counts[0] * price.input + counts[2] * price.cached + counts[1] * price.output) / 1_000_000
    }
}
