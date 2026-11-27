### The Inbox card that asks you to allow files to be written for Set Up or the Cost Profile, one row for each file.
### The maintainer reviews every change to this file, in every language.

## What the request does

# What a Set Up request does: write Notchling's files. $count is how many files are listed below.
consent-install-files = { $count ->
    [one] Set Up writes { $count } file for Notchling
   *[other] Set Up writes { $count } files for Notchling
}
# What a request does: turn on the Antigravity sidecar in Antigravity's config.json.
consent-install-sidecar = Turn on the Antigravity sidecar in Antigravity's config.json

## One file

# Says a file would be created. $path is the file's path.
consent-install-part-create = Create
# Says a file would be changed.
consent-install-part-update = Update
# Says a file would be removed.
consent-install-part-remove = Remove
# Says an earlier file would be put back.
consent-install-part-restore = Put back
# Title of a file row. $change is what happens to the file, such as Update; $path is the file's path.
consent-install-part-title = { $change } { $path }
# How many lines a file change adds and removes. $added and $removed are numbers.
consent-install-part-counts = +{ $added } -{ $removed }
# The start of the checksum the file would have. $hash is 8 characters.
consent-install-part-hash = sha256 { $hash }
# Said under a file row: another file that changes with it. $path is that file's path.
consent-install-part-also = Also changes { $path }
# One field of the Cost Profile that would change. $field is the field's name; $before and $after are its values.
consent-install-part-field = { $field }: { $before } to { $after }
# Said under a list of files: a backup is kept and Notchling only touches what is its own.
consent-install-backup = A backup is kept. Only Notchling's own entries change.

## After Allow

# Shown when the request is out of date and nothing was written.
consent-install-stale-request = Things changed since this request. Nothing was written. Ask again.
# Shown when files changed after the request and were skipped. $paths is a comma separated list.
consent-install-stale-parts = Changed since this request, so not written: { $paths }
# Shown when files could not be written. $paths is a comma separated list.
consent-install-part-failed = Could not write: { $paths }
# Shown on a card when no file is ticked, so Allow is off.
consent-install-none-kept = Tick at least one file to allow this.
