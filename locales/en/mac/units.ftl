### Ages, durations, dates, compact numbers and the list separator, on the Mac.

## Ages

# How long ago a session last did something, in the Sidebar. Under a minute reads as now.
units-age-now = now
# $n is whole minutes; the m stands for minutes.
units-age-minutes = { NUMBER($n) }m
# $n is whole hours; the h stands for hours.
units-age-hours = { NUMBER($n) }h
# $n is whole days; the d stands for days.
units-age-days = { NUMBER($n) }d

## Durations

# How long a step or a reply took. $n is whole seconds.
units-duration-seconds = { NUMBER($n) }s
# $minutes is whole minutes; $seconds is the seconds written with two digits, such as 05.
units-duration-minutes = { NUMBER($minutes) }m { $seconds }s
# $hours is whole hours and $minutes the whole minutes after them.
units-duration-hours = { NUMBER($hours) }h { NUMBER($minutes) }m

## Compact numbers

# A token count in thousands. $value is the number already written, such as 12.4.
units-thousands = { $value }k
# A token count in millions. $value is the number already written, such as 1.2.
units-millions = { $value }M

## Clock

# The day of the week, short, in a limit's reset time.
units-weekday-sun = Sun
units-weekday-mon = Mon
units-weekday-tue = Tue
units-weekday-wed = Wed
units-weekday-thu = Thu
units-weekday-fri = Fri
units-weekday-sat = Sat
# The twelve-hour clock's halves of the day.
units-am = AM
units-pm = PM
# A reset time such as Fri 9:00 AM. $minute is written with two digits.
units-reset-clock = { $weekday } { NUMBER($hour) }:{ $minute } { $suffix }

## Lists

# Between the items of a list. Keep the space after the comma, or write the separator your language uses.
units-list-separator = ,{" "}
