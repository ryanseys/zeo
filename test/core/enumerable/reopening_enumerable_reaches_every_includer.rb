# Enumerable is a RUST-implemented builtin, yet reopenable (D3): an added
# method registers as a value method on the module id and the MRO walk
# finds it for every includer, its body free to drive the native
# Enumerable protocol (`reduce`) on the receiver.

module Enumerable
  def my_join
    reduce("") { |acc, x| acc + x.to_s }
  end
end
puts [1, 2, 3].my_join
puts (1..3).my_join
__END__
123
123
