# Shellwords -- the vendored pure-Ruby stdlib gem (gems/shellwords), compiled
# from its real upstream source. Its module methods are defined with
# `module_function`, then aliased to shorter names inside `class << self`
# (`alias split shellsplit`) -- class-method aliases resolved against the
# singleton table. rubygems parses build flags with it.
require "shellwords"

puts Shellwords::VERSION

# split / shellsplit: tokenize a command line the way a Bourne shell does.
p Shellwords.split('here are "two words"')
p Shellwords.shellsplit("ruby prog.rb | less")

# escape / shellescape: quote one argument for safe shell interpolation.
puts Shellwords.escape("a b'c")
puts Shellwords.escape("plain")

# join / shelljoin: the inverse -- escape and space-join an argv.
puts Shellwords.join(["echo", "a b", "c"])

# The String refinements the gem also ships.
p "one two three".shellsplit
puts "a b".shellescape
