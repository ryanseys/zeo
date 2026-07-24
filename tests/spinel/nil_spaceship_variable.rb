# nil <=> nil is 0 (Integer), including when the nil is held in a variable
# (a nil-only local widens to a boxed poly value, so this exercises the
# poly <=> path, not the literal-nil fold).
a = nil; b = nil
p(a <=> b)
p((a <=> b).class)
