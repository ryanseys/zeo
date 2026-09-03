# Overriding an EXISTING native method: the user `length` wins at a static
# call site (where `try_collection_dispatch` would answer 3), at a dynamic
# Poly site, and `size` -- a separate method in real Ruby, NOT an alias of
# the override -- stays native.

class String
  def length
    42
  end
end

s = "abc"
puts s.length
puts [s].first.length
puts "xy".size
__END__
42
42
2
