# `d[0] += 1` desugars to a read (fine on a frozen array) then a `[]=`
# (raises) -- real Ruby's own order.

d = [1]
d.freeze
begin
  d[0] += 1
rescue FrozenError => e
  puts e.send(:message)
end
__END__
can't modify frozen Array: [1]
