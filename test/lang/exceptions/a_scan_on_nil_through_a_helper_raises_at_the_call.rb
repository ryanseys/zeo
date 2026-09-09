# A helper that calls `scan` on its argument, reached with nil, raises
# NoMethodError at the call rather than at compile time -- the each over its
# result is compiled but never runs.
# (spinel issue #3147)
def wrap(value)
  tokens = tokenize(value)
  tokens.each { |word| }
  :done
end

def tokenize(value) = value.scan(/\S+|\n/)

begin
  wrap(nil)
rescue NoMethodError
  puts "raised NoMethodError"
end
puts "ok"
__END__
raised NoMethodError
ok
