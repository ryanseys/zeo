# NilClass#rationalize (and #to_r) return (0/1), including when the nil is
# held in a variable (a nil-only local widens to a boxed poly value).
r = nil
p(r.rationalize)
p(r.to_r)
p(nil.rationalize)
