# Choosing a subagent model

When you spawn a subagent and no pstack role line or skill names its model, pick `model` and `effort` for the job. Claude Code takes `model` as `haiku`, `sonnet`, `opus` or `fable` and `effort` as `low`, `medium`, `high`, `xhigh` or `max`.

| Model | Price in/out per 1M tokens | Use for |
| --- | --- | --- |
| haiku (Haiku 5.5) | $0.10 / $0.50 | Bulk reading, search, listing, log scanning, running a check and reporting the result. |
| sonnet (Sonnet 5.5) | $2 / $10 | Routine code: specified edits, tests, small features, mechanical refactors, git history digs. |
| opus (Opus 5.5) | $4 / $20 | Judgment: design, review, root-causing, prose, synthesis of other agents' reports. |
| fable (Fable 5.1) | $10 / $50 | The hardest work only: cross-cutting design, subtle concurrency or algorithms, a bug two fixes failed to kill. Turns run long. |

All four have a 1M context window.

## Effort

- `low` or `medium` for lookups and mechanical work. Lower effort means fewer tool calls and shorter answers.
- `high` for normal coding and review.
- `xhigh` for long agentic work and anything intelligence-sensitive.
- `max` only when correctness matters more than cost and the level below fell short.
- Try a stronger model at lower effort before a weaker model at high effort. On current models, lower effort often matches the previous generation at high.

## Rules

- Judge cost per finished task, not per call. A cheap agent that needs a retry costs more.
- Escalate one step (haiku to sonnet, sonnet to opus, opus to fable) when an agent fails the same task twice, and say so.
- For a second opinion, use a different model from the first. Agreement across models is the signal.
- Say which model and effort you picked for each spawn and why, in one line.


## Reviews and second opinions

Code reviews and builder checks go to another tool through Notchling (`notchling delegate`), not to an opus or fable review subagent. Which tool and model to use is Notchling's knowledge, not this file's: follow the `notchling-delegate` skill and `notchling delegate-models`. If delegation fails, fall back to one sonnet review subagent at effort high, and say so.
