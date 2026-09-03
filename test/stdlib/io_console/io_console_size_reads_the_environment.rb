# `require "io/console/size"` is its own require in ruby -- `io/console`
# alone does NOT define `IO.console_size` -- and it was a LoadError here, so
# irb, debug and power_assert all failed on their first line.

p IO.respond_to?(:console_size)
p IO.respond_to?(:default_console_size)

require "io/console/size"

p IO.respond_to?(:console_size)
p IO.respond_to?(:default_console_size)

ENV["LINES"] = "37"
ENV["COLUMNS"] = "111"
p IO.default_console_size

# A blank or non-numeric value falls back, because `String#to_i.nonzero?`
# answers nil for both.
ENV["LINES"] = "0"
ENV["COLUMNS"] = "nope"
p IO.default_console_size

ENV.delete("LINES")
ENV.delete("COLUMNS")
p IO.default_console_size

# The size require pulls `io/console` in with it.
r, w = IO.pipe
p r.respond_to?(:echo?)
r.close
w.close
__END__
false
false
true
true
[37, 111]
[25, 80]
[25, 80]
true
