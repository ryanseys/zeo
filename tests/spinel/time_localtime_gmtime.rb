# Time#gmtime / #localtime (and the getutc / getlocal variants) flip the
# UTC-presentation flag. The underlying instant is unchanged, so these
# checks stay timezone-independent.
t = Time.utc(2000, 1, 2, 3, 4, 5)

# gmtime / getutc present the instant in UTC.
p t.gmtime.year
p t.gmtime.hour
p t.getutc.hour
p t.getutc.min

# localtime changes the presentation, but flipping back to UTC restores
# the original broken-down fields (the instant is preserved).
p t.localtime.utc.hour
p t.localtime.getutc.year

# The epoch second is invariant across presentation flips.
p t.to_i == t.localtime.to_i
p t.to_i == t.gmtime.to_i
