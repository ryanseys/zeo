# Time#to_s renders the "YYYY-MM-DD HH:MM:SS +ZZZZ" form (no subsecond
# fraction). Use .utc for a timezone-independent, deterministic string.
t = Time.at(1700000000).utc
puts t.to_s
puts t.to_s.length > 0
