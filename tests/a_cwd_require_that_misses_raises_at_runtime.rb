# `require "./x"` resolves against the runtime working directory in real
# Ruby. zeo resolves it against the requiring file's directory (the
# deterministic analogue -- `load`'s documented rule), and a MISS defers to
# a catchable runtime `LoadError` instead of refusing to compile: the gems
# that write this shape only ever worked when run from the directory the
# path assumes, and wrap it in a rescue for everywhere else.
begin
  require "./tools/not_shipped_here"
rescue LoadError => e
  puts "caught: #{e.class}"
end

begin
  require "~/nothing_under_home_either"
rescue LoadError => e
  puts "caught: #{e.class}"
end

puts "still compiling"
