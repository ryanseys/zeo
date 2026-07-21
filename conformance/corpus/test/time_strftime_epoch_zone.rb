t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.strftime("%z")   # zone offset
p t.strftime("%s")   # seconds since epoch
p t.to_i             # correct epoch seconds, for comparison
