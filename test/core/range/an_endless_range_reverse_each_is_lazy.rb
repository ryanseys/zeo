# `(1..).reverse_each` answers an Enumerator (the raise comes only when it
# is iterated); zeo raises TypeError at the call. (Found by the 2026-08-24
# probe sweep.)
e = (1..).reverse_each
puts e.class
begin
  e.next
rescue TypeError => err
  puts err.message
end
__END__
Enumerator
can't iterate from NilClass
