# `$*` is an alias of ARGV. zeo maps ARGV but leaves $* an ordinary
# (nil-valued) global.
puts $*.inspect
puts ARGV.inspect
puts ($*.equal?(ARGV)).inspect
