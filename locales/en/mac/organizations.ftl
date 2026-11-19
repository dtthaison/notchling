### Settings > Organizations: the page that lists the organizations that own accounts, secrets and machines, and where each one is renamed, given a colour and folders, or deleted.
### An organization is a name Notchling groups a person's work under, such as Personal or a company. Its folders are folders on this Mac whose sessions belong to it.

## Page

# Tab of the Settings sheet: the list of organizations.
organizations-page = Organizations

# Section title above the list of organizations on that page.
organizations-section = Organizations

# Note under the section title on that page.
organizations-intro = Each account, secret and machine belongs to one organization. Everything starts in Personal.

# Note under the list while Personal is the only organization.
organizations-only-personal = With only Personal, nothing else in { -brand-notchling } shows organizations. Add one to keep accounts, secrets and machines apart.

# Shown on the page while the daemon has not answered.
organizations-loading = Reading organizations…

# Shown on the page when the app has no connection to the daemon.
organizations-detached = The daemon isn't running, so organizations can't be read.

# Shown on the page when the list could not be read; $why is the reason in English.
organizations-read-failed = Organizations couldn't be read: { $why }

# Shown on the page when the stored organizations can't be read, so no change is allowed; $why is the reason in English.
organizations-unreadable = Organizations can't be read, so nothing here can be changed. { $why }

## Buttons

# Button that opens the form for a new organization. Keep the ellipsis.
organizations-new = New Organization…

# Title of the form for a new organization.
organizations-new-title = New Organization

# Placeholder in the box for an organization's name.
organizations-name-placeholder = Name

# Button that makes the new organization.
organizations-create = Create

# Button that saves a new name.
organizations-save = Save

# Button that closes a form without changing anything.
organizations-cancel = Cancel

## Row

# The line under an organization's name; $accounts, $secrets and $machines are the three counts below.
organizations-counts = { $accounts } · { $secrets } · { $machines }

# Count of accounts in an organization; $count is a number.
organizations-count-accounts = { $count ->
    [one] { NUMBER($count) } account
   *[other] { NUMBER($count) } accounts
}

# Count of secrets in an organization; $count is a number.
organizations-count-secrets = { $count ->
    [one] { NUMBER($count) } secret
   *[other] { NUMBER($count) } secrets
}

# Count of machines in an organization; $count is a number.
organizations-count-machines = { $count ->
    [one] { NUMBER($count) } machine
   *[other] { NUMBER($count) } machines
}

# Line under the counts of an organization that has folders; $folders is its folders, such as ~/work/acme, separated by commas.
organizations-folders-line = Folders: { $folders }

# Line under the counts of an organization that has no folders.
organizations-folders-none = No folders

## Row menu

# Menu item: change the organization's name. Keep the ellipsis.
organizations-menu-rename = Rename…

# Menu item: choose the organization's color.
organizations-menu-colour = Colour

# Menu item: edit the folders whose sessions belong to the organization. Keep the ellipsis.
organizations-menu-folders = Folders…

# Menu item: remove the organization.
organizations-menu-delete = Delete Organization

## Colour

# Title of the row of colors to choose from; $name is the organization's name.
organizations-colour-title = Colour of { $name }

# Name of a color an organization can have.
organizations-colour-grey = Grey

# Name of a color an organization can have.
organizations-colour-orange = Orange

# Name of a color an organization can have.
organizations-colour-yellow = Yellow

# Name of a color an organization can have.
organizations-colour-lime = Lime

# Name of a color an organization can have.
organizations-colour-teal = Teal

# Name of a color an organization can have.
organizations-colour-blue = Blue

# Name of a color an organization can have.
organizations-colour-purple = Purple

# Name of a color an organization can have.
organizations-colour-pink = Pink

# Name of a color an organization can have.
organizations-colour-red = Red

## Folders

# Title of the folder editor; $name is the organization's name.
organizations-folders-title = Folders of { $name }

# Note under that title; $name is the organization's name.
organizations-folders-note = Sessions started in these folders, and in folders inside them, belong to { $name }. When two organizations have nested folders, the longer folder wins.

# Placeholder in the box for a folder. The example is a path and stays as it is.
organizations-folders-placeholder = Folder, such as ~/work/acme

# Button that adds the typed folder to the list.
organizations-folders-add = Add

# Shown in the folder editor while the list is empty.
organizations-folders-empty = No folders yet

# Shown in the folder editor while the daemon works out what a change would do.
organizations-folders-checking = Checking what this would change…

# Shown in the folder editor when no open session would change organization.
organizations-folders-moved-none = No open session changes organization.

# Heading over the open sessions that would change organization; $count is how many.
organizations-folders-moved = { $count ->
    [one] One open session would change organization:
   *[other] { $count } open sessions would change organization:
}

# One open session in that list; $folder is its folder, $from and $to are organization names.
organizations-folders-moved-row = { $folder }: { $from } to { $to }

# Last line of that list when more sessions would change than are shown; $count is how many are left out.
organizations-folders-moved-more = and { $count } more

# Note under a folder that lies inside another organization's folder; $folder is the new folder, $other the other folder, $name the other organization.
organizations-folders-inside = { $folder } is inside { $other }, which belongs to { $name }. Under { $folder } the longer folder wins.

# Note under a folder that holds another organization's folder; $folder is the new folder, $other the other folder, $name the other organization.
organizations-folders-contains = { $folder } holds { $other }, which belongs to { $name }. Sessions under { $other } stay with { $name }.

## Delete

# Title of the question before an empty organization is deleted; $name is its name.
organizations-delete-title = Delete { $name }?

# Note under that question; $name is the organization's name.
organizations-delete-note = { $name } is empty, so no account, secret or machine is lost. Its folders stop belonging to it.

# Button that deletes the organization.
organizations-delete-confirm = Delete

# Said when Personal is asked to be deleted.
organizations-error-personal = Personal can't be deleted. It is where everything starts.

# Said when an organization that still holds things is asked to be deleted; $name is its name, $things is a list such as 2 accounts and 1 secret.
organizations-error-not-empty = { $name } still has { $things }. Move them to another organization first.

## Problems

# Said when a name is missing.
organizations-error-name-empty = Give the organization a name.

# Said when a name is too long; $max is the most characters allowed.
organizations-error-name-too-long = Names are { $max } characters at most.

# Said when a name is in use; $name is the name.
organizations-error-name-taken = Another organization is already called { $name }.

# Said when a color is not one of the choices; $color is what was sent.
organizations-error-colour = { $color } isn't one of the colours.

# Said when a folder is not a full path; $folder is what was typed.
organizations-error-folder-invalid = { $folder } isn't a full folder. Start it with / or ~/.

# Said when a folder already belongs to another organization; $folder is the folder, $name the organization.
organizations-error-folder-taken = { $folder } already belongs to { $name }.

# Said when the organization is no longer there.
organizations-error-gone = That organization is gone. The list was read again.

# Said when a change failed for another reason; $why is the reason in English.
organizations-error-failed = The change wasn't made: { $why }
