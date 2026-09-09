# instance_of? is true only for the exact class; is_a? is true up the hierarchy.
# (spinel issue #3013)
p ArgumentError.new.instance_of?(StandardError)
p ArgumentError.new.instance_of?(ArgumentError)
p ArgumentError.new.instance_of?(Exception)
p ArgumentError.new.is_a?(StandardError)
p RuntimeError.new.instance_of?(RuntimeError)
p RuntimeError.new.instance_of?(StandardError)
begin
  raise TypeError, "x"
rescue => e
  p e.instance_of?(TypeError)
  p e.instance_of?(StandardError)
end
__END__
false
true
false
true
true
false
true
false
