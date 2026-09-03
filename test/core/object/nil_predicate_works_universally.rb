# `.nil?` on Poly, builtin, and Object receivers, with a user override
# winning.

puts nil.nil?
puts 1.nil?
class Once
  def check(x)
    x.nil?
  end
end
puts Once.new.check(nil)
puts Once.new.check(5)
puts Once.new.nil?
__END__
true
false
true
false
false
