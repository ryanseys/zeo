# Every `loop` desugars to a `rescue StopIteration` whose binding must be
# ITS OWN local. A single fixed name (`__loop_stop`) aliased every loop in
# the program to one local -- a `Ractor.new { loop { ... } }` written inside
# a method that also used `loop` looked like it captured the enclosing
# method's binding, and refused isolation over a name the source never
# wrote. (The fixed name also leaked into `local_variables`.)
def outer_uses_loop
  count = 0
  loop do
    count += 1
    break if count == 2
  end
  r = Ractor.new do
    total = 0
    loop do
      total += 1
      break if total == 3
    end
    total
  end
  puts r.value
  puts count
end

outer_uses_loop

# `loop` returns the StopIteration's result through its binding -- the
# binding the desugar names must still resolve per loop.
e = [1, 2].each
first = loop do
  e.next
end
puts first.inspect

puts local_variables.grep(/__loop/).length
__END__
3
2
[1, 2]
0
#@ stderr
lang/control/each_loop_binds_its_own_stop.rb:13: warning: Ractor API is experimental and may change in future versions of Ruby.
