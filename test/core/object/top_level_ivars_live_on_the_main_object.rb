# Was rejected as "the `main` object has no ivar storage". It has some
# now (`dispatch::Object`'s name-keyed map), so a top-level `@x` -- read
# or written, at the top level or from a top-level `def`, which is a
# private method of Object whose self IS main -- is ordinary state.
# A never-assigned one reads nil rather than raising.

@x = 1
p @x
p @never
def read_it; @x; end
p read_it
def bump; @count = (@count || 0) + 1; end
bump
bump
p @count
__END__
1
nil
1
2
