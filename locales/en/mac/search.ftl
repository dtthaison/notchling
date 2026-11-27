### The search panel (⌘⇧P): the headings that group its results.

## Group headings

# Heading above the pages in the search panel's results. Title Case.
search-group-pages = Pages
# Heading above the saved logins (Vault entry names) in the search panel's results. Title Case.
search-group-vault = Vault
# Heading above the sessions in the search panel's results. Title Case.
search-group-sessions = Sessions

## Facet chips

# Chip for the filter when:today in the search panel. Title Case.
search-chip-today = Today
# Chip for the filter when:7d: the last days. Title Case.
search-chip-days = { $count ->
    [one] Last Day
   *[other] Last { $count } Days
}
# Chip for the filter when:2026-11: a month and its year, for example November 2026. Title Case.
search-chip-month = { $month } { $year }
# Chip for a typed facet value nothing has, for example No Such Project. The name is Tool, Project, Account, Section, Owner, Kind or State. Title Case.
search-chip-unknown = No Such { $name }
# Chip for a category the Search panel has no results for yet, for example Not Searched: Settings. The name is a category. Title Case.
search-chip-not-searched = Not Searched: { $name }
# Name of the tool facet (the app a session ran in), used in chips. Title Case.
search-facet-tool = Tool
# Name of the project facet (a folder), used in chips. Title Case.
search-facet-project = Project
# Name of the account facet (who a session ran as), used in chips. Title Case.
search-facet-account = Account
# Name of the section facet (a Settings section), used in chips. Title Case.
search-facet-section = Section
# Name of the owner facet (who saved a page), used in chips. Title Case.
search-facet-owner = Owner
# Name of the kind facet, used in chips. Title Case.
search-facet-kind = Kind
# Name of the state facet, used in chips. Title Case.
search-facet-state = State
# Chip for tool:claude. Product name.
search-tool-claude = Claude
# Chip for tool:codex. Product name.
search-tool-codex = Codex
# Chip for tool:agy. Product name.
search-tool-agy = Antigravity
# Month name for the when:2026-01 chip. Title Case.
search-month-january = January
# Month name for the when:2026-02 chip. Title Case.
search-month-february = February
# Month name for the when:2026-03 chip. Title Case.
search-month-march = March
# Month name for the when:2026-04 chip. Title Case.
search-month-april = April
# Month name for the when:2026-05 chip. Title Case.
search-month-may = May
# Month name for the when:2026-06 chip. Title Case.
search-month-june = June
# Month name for the when:2026-07 chip. Title Case.
search-month-july = July
# Month name for the when:2026-08 chip. Title Case.
search-month-august = August
# Month name for the when:2026-09 chip. Title Case.
search-month-september = September
# Month name for the when:2026-10 chip. Title Case.
search-month-october = October
# Month name for the when:2026-11 chip. Title Case.
search-month-november = November
# Month name for the when:2026-12 chip. Title Case.
search-month-december = December

## Meaning

# Small tag on a search result that matched by meaning, not by the words typed. Title Case.
search-related = Related
# Footer of the search panel while the index by meaning is being built. $done and $total are whole numbers of passages.
search-meaning-progress = Indexing { NUMBER($done) } of { NUMBER($total) } passages
