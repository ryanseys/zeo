t = Time.utc(2026, 7, 16, 13, 45, 30) + 0.123456789
p t.strftime("%P")   # lowercase am/pm
p t.strftime("%L")   # milliseconds
p t.strftime("%6N")  # fractional seconds, 6 digits
p t.strftime("%^A")  # upcase flag
p t.strftime("%3S")  # width modifier
p t.strftime("%+")   # not a Time#strftime directive in CRuby
p t.strftime("%10Y") # width modifier on %Y
p Time.utc(2026, 7, 16).strftime("%:z")  # colon offset
