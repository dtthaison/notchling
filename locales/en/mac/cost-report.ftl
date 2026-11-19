### The Cost Check report that `notchling cost report` prints: what the person's agents spent and what to change.
### Dollar amounts are written by the program with a currency sign; a share is a whole percent with its sign.

## Heading

# The first line of the report. $days is how many days it covers.
cost-report-title = { $days ->
    [one] Cost Check: the last day
   *[other] Cost Check: the last { $days } days
}
# The spend line. $money is an amount, $calls and $tokens are counts, $label is the next message.
cost-report-spend = Spend: { $money } over { $calls } calls and { $tokens } tokens. { $label }
# Said when the plan is an API key: the dollar figures are close to the bill.
cost-report-label-estimate = Dollar figures are estimates at list prices.
# Said when the plan is a subscription or not known: the dollar figures are not a bill.
cost-report-label-api-equivalent = Dollar figures are API-equivalent: what these tokens would cost at list prices. They are not your bill.
# The spend line when no model in the window had a price.
cost-report-spend-unknown = Spend: unknown, because no model used had a price. { $calls } calls and { $tokens } tokens.
# $count is how many calls used a model with no price.
cost-report-unpriced = { $count } calls used a model with no price. They are counted as unknown, not as zero.
# Said when the first reading of the transcripts has not finished.
cost-report-partial = Notchling is still reading older transcripts, so this report is not complete yet. Run it again in a minute.
# The word for a share or amount that could not be worked out.
cost-report-unknown = unknown

## Shares

# The heading over where the spend goes.
cost-report-shares-heading = Where it goes
# $share is a percent with its sign, $calls a count.
cost-report-share-main = Main chats: { $share } of spend, { $calls } calls
# $share is a percent with its sign, $calls a count.
cost-report-share-sub = Helper agents: { $share } of spend, { $calls } calls
# One model's line. $model is the model's name.
cost-report-share-model = { $model }: { $share } of spend, { $calls } calls

## Cache

# $who is Main chats or Helper agents; the counts are sessions or runs by the cache time their transcripts show.
cost-report-cache = Cache time read from the transcripts, { $who }: 1 hour { $one_hour }, 5 minutes { $five_min }, both { $mixed }, unknown { $unknown }
# A name used in the cache line.
cost-report-who-main = main chats
# A name used in the cache line.
cost-report-who-sub = helper agents

## Findings

# The heading over the list of changes.
cost-report-findings-heading = What to change, biggest first
# Said when no lever is worth the effort.
cost-report-none = Nothing stands out: no change is worth the effort.
# One finding with an amount. $n is its place, $title its name, $saving an amount, $share a percent with its sign.
cost-report-saving = { $n }. { $title }: about { $saving } less ({ $share } of spend)
# One finding for a subscription. $share is a percent of usage; $saving is API-equivalent.
cost-report-saving-subscription = { $n }. { $title }: about { $share } of your usage ({ $saving } API-equivalent)
# One finding whose saving cannot be worked out because a price is missing.
cost-report-saving-unknown = { $n }. { $title }: saving unknown, a price is missing
# How many tokens a finding would avoid. $tokens is a compact count.
cost-report-tokens = About { $tokens } fewer tokens.
# Said after a finding's evidence when a helper 1-hour cache is weighed.
cost-report-hour-cache = A 1-hour helper cache would add { $added } in cache writes and save { $saved } in rebuilds, so { $verdict }.
# Used in the sentence about the 1-hour helper cache.
cost-report-verdict-pays = it pays
# Used in the sentence about the 1-hour helper cache.
cost-report-verdict-not = it does not pay

## Finding names

# A finding's name.
cost-report-name-premium-subagents = Opus or Fable on helper agents
# A finding's name.
cost-report-name-repeated-reviews = Repeated reviews
# A finding's name.
cost-report-name-cold-resumes = Cache rebuilt after an idle gap
# A finding's name.
cost-report-name-model-switches = Cache rebuilt after a model switch
# A finding's name.
cost-report-name-long-runs = Long helper runs and big contexts
# A finding's name.
cost-report-name-many-builders = Many builders at once
# A finding's name.
cost-report-name-main-effort = Main chat effort

## Evidence

# $runs and $calls are counts, $models and $target are model names, $cost an amount, $share a percent.
cost-report-ev-premium-subagents = { $runs } helper runs ({ $calls } calls) on { $models } cost { $cost }, { $share } of helper spend. { $target } would do routine work for less. Keep the strong models for design, root causes and security reviews.
# $runs, $repeat, $sessions and $most are counts, $cost an amount.
cost-report-ev-repeated-reviews = { $runs } review runs; { $repeat } were repeats in the same session ({ $sessions } sessions, up to { $most } in one), costing { $cost }. Review once at the end, and re-review only the fix.
# $count, $main and $sub are counts, $cost an amount, $minutes the average wait in minutes.
cost-report-ev-cold-resumes = { $count } rebuilds after a wait longer than the cache time ({ $main } in main chats, { $sub } in helpers) cost { $cost } more than reading the cache. The average wait was { $minutes } minutes. Keep waits short, or resume within the cache time.
# $count is a count, $tokens a compact count, $cost an amount.
cost-report-ev-model-switches = { $count } model switches inside the cache time re-wrote { $tokens } tokens, { $cost } more than reading them. Switch at a task boundary, or in a new chat.
# $over, $limit, $max, $p90 and $big are counts, $top a percent.
cost-report-ev-long-runs = { $over } helper runs went past { $limit } calls (longest { $max }, 90th percentile { $p90 }), { $big } calls carried more than 150k tokens of context, and the top tenth of runs is { $top } of helper spend. Hand back at about { $limit } calls with a handoff file.
# $peak, $buckets, $cap and $crowd are counts, $cost an amount, $share a percent.
cost-report-ev-many-builders = Up to { $peak } helpers worked at once. { $buckets } ten-minute spans had more than { $cap } ({ $cost } of spend), and { $share } of all spend happened with { $crowd } or more agents active. Run at most { $cap } at once.
# $share is a percent, $calls a count.
cost-report-ev-main-effort = { $share } of main-chat calls already use low or medium effort ({ $calls } calls).

## Not worth changing

# The heading over the levers that are too small to bother with.
cost-report-notworth-heading = Not worth changing
# $title is a finding's name and $saving an amount.
cost-report-notworth-saving = { $title }: saves only about { $saving }
# $title is a finding's name.
cost-report-notworth-already = { $title }: already low

## Not measurable

# The heading over what the transcripts could not tell.
cost-report-unknown-heading = Could not be measured
# $count is how many calls.
cost-report-unknown-price = { $count } calls used a model with no price
# $count is how many calls.
cost-report-unknown-cache-ttl = { $count } calls were in sessions whose cache time could not be read
# $count is how many calls.
cost-report-unknown-agent-type = { $count } helper calls had no agent type
# $count is how many calls.
cost-report-unknown-effort = { $count } main-chat calls had no recorded effort
