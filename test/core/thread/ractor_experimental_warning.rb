# The Ractor API is experimental, and Ruby says so once -- at the FIRST
# `Ractor.new`, naming that line, and never again however many Ractors follow.
# It is an ordinary categorised warning, so `Warning[:experimental] = false`
# silences it. See the sidecar golden for the one line this prints.

puts "before"

# The first one warns, naming ITS line (7 below), not this comment's.
first = Ractor.new { 1 }
puts first.value

# Every later one is silent.
second = Ractor.new { 2 }
third = Ractor.new(3) { |n| n }
puts second.value
puts third.value

# The category flag is readable and writable like any other.
p Warning[:experimental]
Warning[:experimental] = false
p Warning[:experimental]
Warning[:experimental] = true

# Still silent -- the notice is once per process, not once per setting.
puts Ractor.new { 4 }.value
__END__
before
1
2
3
true
false
4
#@ stderr
core/thread/ractor_experimental_warning.rb:9: warning: Ractor API is experimental and may change in future versions of Ruby.
