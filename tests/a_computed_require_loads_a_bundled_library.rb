# A `require` whose argument is not a literal has to reach the libraries the
# runtime carries, not just the files on `$LOAD_PATH`.
#
# zeo folds a LITERAL `require "date"` at compile time -- the native half is
# linked in and registered before line 1, so the fold emits nothing but a
# loaded-feature marker. A computed target names no feature the compiler can
# see, so it reaches `Kernel#require` at run time instead, and that path used
# to search only the disk. `$LOAD_PATH` is empty by default and a bundled
# library is inside the binary rather than on it, so every one of these raised
# `LoadError`.
#
# `%w[...].each { |f| require f }` over stdlib names is the idiom RubyGems,
# Bundler and Rails all write, so the blast radius was most of the ecosystem.

names = %w[date set stringio strscan pathname time bigdecimal
           zlib socket monitor objspace digest etc fcntl]
names.each { |f| puts "#{f}\t#{require f}" }

# Ruby's boolean, both halves: true the first time, false for a feature
# already loaded -- and false even on the FIRST require of one ruby loads
# before line 1.
puts "second time"
names.each { |f| puts "#{f}\t#{require f}" }

# Every spelling of a non-literal argument reaches the same place.
one = "json"
puts "variable\t#{require one}"
puts "interpolated\t#{require "#{'string'}io"}"
puts "method result\t#{require ["etc"].first}"

# A name nothing provides still raises, and the message is ruby's.
begin
  missing = "no_such_library_anywhere"
  require missing
rescue LoadError => e
  puts "missing\t#{e.message}"
end
