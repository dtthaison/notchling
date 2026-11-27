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

## Message times

# How long ago a message was sent, in its row. Under a minute reads as just now.
units-just-now = just now
# $count is whole minutes.
units-minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
# $count is whole hours.
units-hours-ago = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
# $time is a clock time such as 9:05 AM.
units-yesterday = Yesterday { $time }
# A date in this year. $month is the short month and $day the day of the month.
units-date-short = { $month } { $day }
# A date in another year. $year is written without a thousands separator.
units-date-year = { $month } { $day }, { $year }
# The full date in a message's tooltip, such as Friday, January 15, 2027 at 9:05 AM.
units-date-full = { $weekday }, { $month } { $day }, { $year } at { $time }
# A clock time on the twelve-hour clock. $minute is written with two digits.
units-clock = { NUMBER($hour) }:{ $minute } { $suffix }

## Months and days

# The short months, in a date.
units-month-jan = Jan
units-month-feb = Feb
units-month-mar = Mar
units-month-apr = Apr
units-month-may = May
units-month-jun = Jun
units-month-jul = Jul
units-month-aug = Aug
units-month-sep = Sep
units-month-oct = Oct
units-month-nov = Nov
units-month-dec = Dec
# The months in full, in a message's tooltip.
units-month-long-jan = January
units-month-long-feb = February
units-month-long-mar = March
units-month-long-apr = April
units-month-long-may = May
units-month-long-jun = June
units-month-long-jul = July
units-month-long-aug = August
units-month-long-sep = September
units-month-long-oct = October
units-month-long-nov = November
units-month-long-dec = December
# The days of the week in full.
units-weekday-long-sunday = Sunday
units-weekday-long-monday = Monday
units-weekday-long-tuesday = Tuesday
units-weekday-long-wednesday = Wednesday
units-weekday-long-thursday = Thursday
units-weekday-long-friday = Friday
units-weekday-long-saturday = Saturday

## Lists

# Between the items of a list. Keep the space after the comma, or write the separator your language uses.
units-list-separator = ,{" "}
