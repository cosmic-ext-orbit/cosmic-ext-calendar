app-title = Calendar
about = About
about-description = Plan events and organize your schedule on the COSMIC desktop.
repository = Repository
about-releases = Releases
about-report-issue = Report an issue
app-menu = App menu
page-id = Page { $num }
git-description = Git commit {$hash} on {$date}

today = Today
month = Month
week = Week
day = Day

create-event = Create event

personal-calendar = Personal

new-event = New Event
edit-event = Edit
summary = Summary
description = Description
location = Location
all-day = All day
task = Task
date = Date
starts = Starts
ends = Ends
recurrence = Repeats
every = Every
on-days = On days
recurrence-ends = Ends
end-never = Never
end-until = On date
end-count = After
occurrences = occurrences
save = Save
cancel = Cancel
delete = Delete
delete-occurrence = Delete this event
delete-series = Delete entire series

import-ics = Import calendar…
export-ics = Export calendar…
new-calendar = New calendar
edit-calendar = Edit calendar
calendars = Calendars
create = Create
color = Color
rename = Rename
search = Search events…
no-results = No matching events

previous-month = Previous month
next-month = Next month
month-day-agenda = Day agenda
month-agenda-date = { $weekday }, { $month } { $day }
month-no-events = No events
month-compact-event-count = +{ $count }
month-more-events = +{ $count } more
month-day-accessible =
    { $count ->
        [one] { $date }, one event
       *[other] { $date }, { $count } events
    }
weekday-mon = Mon
weekday-tue = Tue
weekday-wed = Wed
weekday-thu = Thu
weekday-fri = Fri
weekday-sat = Sat
weekday-sun = Sun
month-jan = January
month-feb = February
month-mar = March
month-apr = April
month-may = May
month-jun = June
month-jul = July
month-aug = August
month-sep = September
month-oct = October
month-nov = November
month-dec = December
start-date = Start date
end-date = End date
invalid-date-range = Choose an end time on or after the start.
invalid-all-day-range = Choose an end date on or after the start date.
event-timezone = Time zone
event-timezone-placeholder = Optional, e.g. America/Phoenix
invalid-event-timezone = Enter a time zone such as America/Phoenix.
nonexistent-event-time = This time is skipped in the selected time zone. Choose a different time.
event-save-error = Could not save the event. Your changes are still in the editor.
event-delete-error = Could not delete the event. { $error }
settings = Settings
settings-date-and-time = Date and time
display-timezone = Display time zone
display-timezone-system = System time zone
recurrence-edit-scope = Apply changes to
recurrence-this-event = This event
recurrence-all-events = All events
ics-import-result = Imported { $imported }. Skipped { $skipped } duplicate or unsupported entries.
ics-import-error = Could not finish importing. { $error }
ics-import-working = Importing calendar…
ics-import-no-calendar = Create a local calendar before importing.
ics-import-destination = Destination calendar
ics-import-confirm = Import
delete-following = Delete this and following
recurrence-following-events = This and following events
event-floating-time = Keep the same time in every time zone
calendar-add-at = Create event on { $date } at { $time }
event-writing = Applying changes…
invalid-recurrence-end = The repeat end date must be on or after the start date.
calendar-copy = Copy
calendar-cut = Cut
calendar-paste = Paste
calendar-duplicate = Duplicate
calendar-paste-error = Could not place the event at that date and time.

delete-calendar = Delete
archive-calendar-confirmation = Delete “{ $name }” from your active calendars? You can restore the calendar and all its events from Archived Calendars.
archived-calendars = Archived Calendars
no-archived-calendars = No archived calendars.
restore-calendar = Restore
calendar-archived = Calendar moved to Archived Calendars.
calendar-archive-error = Could not change the calendar archive. { $error }
calendar-storage-load-error = Some calendar data could not be loaded. Changes are disabled until the storage problem is fixed and the app is restarted. { $error }
default-calendar-error = Could not create the Personal calendar. { $error }
calendar-create-error = Could not create the calendar. { $error }
calendar-save-error = Could not save the calendar. { $error }
calendar-visibility-error = Could not change calendar visibility. { $error }
calendar-export-error = Could not export calendar events. { $error }
display-timezone-save-error = Could not save the display time zone. { $error }
appearance = Appearance
theme = Theme
theme-match-desktop = Match desktop
theme-dark = Dark
theme-light = Light
theme-save-error = Could not save the theme. { $error }

calendar-name = Calendar name
calendar-color = Calendar color
calendar-color-swatch = Color { $color }
calendar-color-custom = Custom color
calendar-color-default = Default color
calendar-color-recent = Recent colors
calendar-color-copy = Copy to clipboard
calendar-color-copied = Copied to clipboard

event-title = Event title
event-date-label = { $date }, { $year }
event-invalid-clock = Enter a valid time in HH:MM format.
event-calendar = Calendar
event-notes = Notes
event-more-options = More options

location-suggestions = Location suggestions
online-location-enable = Enable online lookup
online-location-disable = Disable online lookup
online-location-save-error = Could not save online location settings.
online-location-unavailable = Online suggestions are unavailable.
online-location-description = Use Photon to look up addresses and places. While enabled, location searches are sent to the server below. The public demo is for light use.
photon-endpoint = Photon server URL
photon-endpoint-invalid = Enter an HTTP or HTTPS Photon server URL.
online-location-update = Update server
online-location-about = About the public server
online-location-attribution = Photon · © OpenStreetMap contributors

event-alerts = Alerts
event-alert-at-start = At time of event
event-alert-custom = Custom…
event-alert-custom-minutes = Minutes before
event-alert-invalid = Enter a valid number of minutes before the event.
event-alert-duplicate = Choose a different time for each alert.
event-alert-minutes =
    { $count ->
        [one] { $count } minute before
       *[other] { $count } minutes before
    }
event-alert-hours =
    { $count ->
        [one] { $count } hour before
       *[other] { $count } hours before
    }
event-alert-days =
    { $count ->
        [one] { $count } day before
       *[other] { $count } days before
    }
event-alert-none = None
event-alert-first = Alert
event-alert-numbered =
    { $suffix ->
        [st] { $number }st Alert
        [nd] { $number }nd Alert
        [rd] { $number }rd Alert
       *[th] { $number }th Alert
    }

notification-untitled-event = Untitled event

accounts = Accounts
manage-accounts = Manage accounts
manage-accounts-description = Add calendar accounts directly in Calendar.
open = Open
accounts-checking = Checking accounts…
accounts-daemon-unavailable = Secure account storage is unavailable.
no-accounts-connected = No calendar accounts are connected.
accounts-app-launch-error = Could not open account settings: {$error}
account-save-error = Could not update account: {$error}
account-action-error = Account error: {$error}
accounts-add = Add account
accounts-sign-in = Sign in with {$provider}
accounts-custom-caldav = Custom CalDAV
accounts-url = Server URL
accounts-username = Username
accounts-password = Password
accounts-connect = Connect
accounts-cancel = Cancel
accounts-reconnect = Reconnect
accounts-remove = Remove
accounts-waiting = Waiting for {$provider} sign-in
accounts-waiting-description = Finish the sign-in in your browser.
accounts-error-unavailable = Secure account storage is unavailable.
accounts-error-upgrade = Accounts needs to be updated.
accounts-error-disabled = This account or its Calendar service is disabled.
accounts-error-needs-reauth = This account needs to be connected again.
accounts-error-access-denied = Calendar access was denied. Check the account’s app permissions.
accounts-error-server-denied = The CalDAV server denied access. Check the account’s server permissions.
accounts-error-google-denied = Google refused CalDAV access. Check that the CalDAV API is enabled for the OAuth project and reconnect the Google account.
accounts-error-google-api-disabled = Enable the CalDAV API in the Google Cloud project, then sync again.
accounts-error-google-scope = Reconnect the Google account and grant Calendar access on Google’s consent screen.
accounts-error-consent-timeout = The sign-in request timed out.
accounts-error-failed = The account request could not be completed.
accounts-error-unsupported = This account is not supported.
accounts-syncing = Syncing calendars…
accounts-retrying = Retrying sync. Last error: { $error }
accounts-no-calendars = No calendars found.
accounts-error-microsoft-denied = Microsoft denied calendar access. Check that the app has delegated Calendars.Read permission, then reconnect the account.

keyboard-shortcuts = Keyboard shortcuts
shortcut-general = General
shortcut-navigation = Navigation
shortcut-selected-event = Calendar events
shortcut-event-editor = Event editor
shortcut-calendar-management = Calendar management
shortcut-quit = Quit
shortcut-refresh = Refresh online calendars
shortcut-close-window = Close window
shortcut-dismiss = Dismiss or clear search
shortcut-close-dialog = Close dialog or drawer
shortcut-previous-day = Previous day
shortcut-next-day = Next day
shortcut-previous-week = Previous week
shortcut-next-week = Next week
shortcut-previous-month = Previous month
shortcut-next-month = Next month
shortcut-previous-year = Previous year
shortcut-next-year = Next year
shortcut-event-details = Open selected event details
shortcut-save-event = Save event
shortcut-toggle-all-day = Toggle all-day event
shortcut-save-calendar = Save calendar
shortcut-confirm-archive = Confirm calendar archive
shortcut-confirm-import = Confirm import
shortcut-toggle-sidebar = Toggle calendars sidebar
shortcut-month-agenda = Open selected day’s agenda in Month view
shortcut-previous-event = Previous event on the selected day
shortcut-next-event = Next event on the selected day

local-calendars = Local calendars
sidebar-online-account = Online account
accounts-no-providers = No calendar providers are available in this build.
accounts-add-title = Add a calendar account
accounts-add-body = Choose a provider, then finish signing in in your browser.
accounts-finish-title = Finish signing in
accounts-finish-body = We opened { $provider } in your browser. Finish signing in there, and the account appears here.
accounts-open-browser = Open browser again
