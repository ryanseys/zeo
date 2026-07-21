# strftime output longer than the old fixed 256-byte buffer was silently
# dropped to "". A roomier buffer now handles realistic long formats
# (lots of literal text, repeated or wide directives). A pathological
# field width still does not fit and yields "" gracefully (no crash);
# that case isn't asserted here because CRuby raises Errno::ERANGE for it.
t = Time.at(0).utc

# %Y-%m-%d rather than %F: MinGW's libc strftime doesn't implement the
# C99 %F directive (it returns ""), and this helper calls strftime(3)
# directly. The buffer fix is what this test exercises, not %F.
puts t.strftime("%Y-%m-%d")
puts t.strftime("%Y-%m-%d %H:%M:%S")

# ~500 bytes of output from a repeated directive
puts t.strftime("%Y-" * 100).length

# ~304 bytes: long literal run followed by a directive
puts t.strftime("x" * 300 + "%Y").length

# empty format stays empty
p t.strftime("")
