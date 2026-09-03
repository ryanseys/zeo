# Before this fix, a negative out-of-range `Array#[]=` index was an
# unconditional Rust `panic!` -- a real `IndexError` (catchable) is
# constructed instead now, and the array is otherwise
# unaffected (an in-range negative-from-end or growing-positive index
# still works exactly as before).

a = [1, 2, 3]
begin
  a[-10] = :x
rescue IndexError => e
  puts "caught"
end
a[5] = :y
puts a.length
puts a[5]
__END__
caught
6
y
