# `StringIO` is meant to be drop-in for an `IO`, which is what makes it the
# standard way to capture output or feed a parser in a test. Three of IO's
# methods are missing or mis-scoped on it:
#
#   * `each_line(chomp: true)` -- the keyword is rejected as a positional arg
#     ("wrong number of arguments (given 1, expected 0)"). `readlines`/`gets`
#     take it too.
#   * `printf` -- resolves to the PRIVATE `Kernel#printf` rather than a public
#     `IO#printf`, so an explicit receiver is refused.
#   * `each_char` -- absent entirely.
#
# The shared cause is that zeo's StringIO carries its own hand-written row set
# rather than inheriting IO's, so a name IO has and StringIO's list omits simply
# is not there. `each_line` and `readlines` DO exist; only their `chomp:`
# keyword is unmodelled.

require 'stringio'

s = StringIO.new("a\nb\n")
p s.each_line(chomp: true).to_a

p StringIO.new("a\nb\n").readlines(chomp: true)

out = StringIO.new
out.printf("%05d", 42)
p out.string

p StringIO.new("ab").each_char.to_a

# What already works, so a fix must not disturb it.
p StringIO.new("a\nb\n").each_line.to_a
p StringIO.new("a\nb").readlines
