# Range#step materialises a typed array -- float when the step or a range bound
# is float -- and iterates faithfully: no float drift, correct exclusivity,
# descending negative steps, and ArgumentError on a zero step (CRuby parity).
# A non-literal step (through a method param) previously truncated to int and
# looped forever; a literal float step tripped clang -Werror.
def s(x); x; end

# --- float steps, no block ---
puts (1.0..2.0).step(0.5).to_a.inspect      # float bounds
puts (1..3).step(0.5).to_a.inspect          # int bounds, float step
puts (1...3).step(0.5).to_a.inspect         # exclusive
puts (2.0..1.0).step(-0.5).to_a.inspect     # descending
puts (1.0..2.0).step(-0.5).to_a.inspect     # wrong direction -> empty
puts (0.0..1.0).step(0.1).to_a.inspect      # drift: 1.0 must be included
puts (1.0..6.4).step(1.8).to_a.inspect      # uneven final step
puts (1.0..2.0).step(s(0.5)).to_a.inspect   # non-literal step (runtime path)

# --- int steps, no block ---
puts (1..10).step(3).to_a.inspect
puts (1...10).step(3).to_a.inspect          # exclusive
puts (10..1).step(-2).to_a.inspect          # descending
puts (10...1).step(-1).to_a.inspect         # descending exclusive
puts (1..10).step(-1).to_a.inspect          # wrong direction -> empty
puts (5..5).step(1).to_a.inspect            # single element

# --- block form (statement position) ---
acc = []; (1..10).step(3) { |i| acc << i }; puts acc.inspect
fac = []; (1.0..2.0).step(0.5) { |x| fac << x }; puts fac.inspect
exc = []; (1...3).step(0.5) { |x| exc << x }; puts exc.inspect

# --- exceptional: a zero step raises ArgumentError ---
begin; (1.0..2.0).step(0).to_a; rescue => e; puts "#{e.class}: #{e.message}"; end
begin; (1.0..2.0).step(0.0).to_a; rescue => e; puts "#{e.class}: #{e.message}"; end
begin; (1..10).step(0).to_a; rescue => e; puts "#{e.class}: #{e.message}"; end
begin; (1..10).step(s(0)) { |i| }; rescue => e; puts "#{e.class}: #{e.message}"; end
__END__
[1.0, 1.5, 2.0]
[1.0, 1.5, 2.0, 2.5, 3.0]
[1.0, 1.5, 2.0, 2.5]
[2.0, 1.5, 1.0]
[]
[0.0, 0.1, 0.2, 0.30000000000000004, 0.4, 0.5, 0.6000000000000001, 0.7000000000000001, 0.8, 0.9, 1.0]
[1.0, 2.8, 4.6, 6.4]
[1.0, 1.5, 2.0]
[1, 4, 7, 10]
[1, 4, 7]
[10, 8, 6, 4, 2]
[10, 9, 8, 7, 6, 5, 4, 3, 2]
[]
[5]
[1, 4, 7, 10]
[1.0, 1.5, 2.0]
[1.0, 1.5, 2.0, 2.5]
ArgumentError: step can't be 0
ArgumentError: step can't be 0
ArgumentError: step can't be 0
ArgumentError: step can't be 0
