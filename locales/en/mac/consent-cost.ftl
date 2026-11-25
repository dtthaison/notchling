### The Inbox card that asks you to allow a change to the Cost Profile or the files it writes. Its file rows use the words in consent-install.ftl.
### The maintainer reviews every change to this file, in every language.

## What the request does

# What a Cost Profile request does: write the files that carry the profile. $count is how many files are listed below.
consent-cost-apply = { $count ->
    [one] The Cost Profile writes { $count } file
   *[other] The Cost Profile writes { $count } files
}
# What a request does: put back the files the Cost Profile changed for a tool. $count is how many files; $tool is the tool's name.
consent-cost-undo = { $count ->
    [one] Put back { $count } file the Cost Profile changed for { $tool }
   *[other] Put back { $count } files the Cost Profile changed for { $tool }
}
# What a request does: change fields of the Cost Profile. $count is how many fields are listed below.
consent-cost-profile = { $count ->
    [one] Change { $count } field of the Cost Profile
   *[other] Change { $count } fields of the Cost Profile
}
