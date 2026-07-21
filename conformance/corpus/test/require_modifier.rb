# A bare-string `require` followed by a trailing modifier (`rescue`,
# `if`, ...) used to strand the modifier during require preprocessing and
# produce a parse error ("unexpected 'rescue'"). The require now degrades
# to a `nil` expression so the modifier attaches and the program parses.
# (json/set are stdlib in the reference Ruby, so the rescue is a moot
# defensive guard there; the program does not use either library.)
require 'json' rescue nil
puts "ok1"
x = 5 rescue nil
puts x
require 'set' if false
puts "ok2"
