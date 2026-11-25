# pstack model configuration. One line per role. Delete a line to fall back to the skill default.
# `inherit-parent` or `auto` as a value: the role runs on the parent chat model (omit Agent `model`). Alias entries in a panel list still count toward its fan-out.
# Tiered by task (see ~/.claude/rules/model-choice.md): haiku for bulk reading, sonnet for routine code, opus for judgment, fable for the hardest work.
# Re-running /pstack:setup-pstack with a budget flattens every effort to one level and drops this tiering.
# budget: custom (tiered)
feature, refactoring: sonnet high
bug-fix: sonnet xhigh
perf-issue: opus high
hillclimb: opus high
judgment and prose: opus xhigh
hardest tasks: fable xhigh
how explorer: haiku high
how explainer: opus high
why investigators: sonnet medium
why synthesizer: opus xhigh
reflect tooling: sonnet high
reflect judgment, divergent, synthesizer: opus xhigh
arena runners: fable xhigh, opus xhigh, sonnet xhigh
arena cross-judge pool: fable xhigh, sonnet xhigh
swarm workers: sonnet medium
architect runners: opus xhigh, fable high, sonnet xhigh
interrogate reviewers: fable xhigh, opus xhigh, sonnet high
