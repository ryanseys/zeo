# shellwords defines its module methods (`module_function`) then aliases
# them inside `class << self` (`alias split shellsplit`) -- a class-method
# alias that resolves against the singleton table, not instance methods.

require "shellwords"
p Shellwords.split('a "b c"')
puts Shellwords.escape("a b")
puts Shellwords.join(["a", "b c"])
__END__
["a", "b c"]
a\ b
a b\ c
