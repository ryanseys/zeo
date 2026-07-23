# Ruby-specific regex semantics backed by the real Oniguruma engine.

# `^`/`$` are line anchors, but a trailing newline does not open a new line,
# so there is no phantom match after it.
puts "a\nb\n".scan(/^/).size          # 2, not 3
puts(/^$/.match?("a\n"))              # false

# Inline `(?m:...)` is Ruby's DOTALL (`.` spans `\n`), and it is scoped to the
# group -- `(?i-m:...)` turns the flag back off.
puts(/(?m:a.c)/ =~ "a\nc")            # 0
puts((/a.c/ =~ "a\nc").inspect)      # nil

# The absence operator `(?~...)` matches any run that does NOT contain the
# subpattern.
puts(/\A(?~foo)\z/.match?("bar"))    # true
puts(/\A(?~foo)\z/.match?("xfooy"))  # false

# Alternation keeps each branch's capture in its own slot.
p(/(a)|(b)|(c)/.match("c").captures) # [nil, nil, "c"]
