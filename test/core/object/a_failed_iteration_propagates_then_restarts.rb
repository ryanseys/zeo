# A mid-iteration exception propagates out of `next`; the NEXT `next`
# re-inits the dead fiber and RESTARTS the iteration (CRuby's
# get_next_values rule, oracle-verified).

e = Enumerator.new do |y|
  y << 1
  raise ArgumentError, "mid"
end
p e.next
begin
  e.next
rescue ArgumentError => ex
  puts "propagated: #{ex.message}"
end
p e.next
__END__
1
propagated: mid
1
