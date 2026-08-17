# The whole-program shape: NO call site anywhere supplies the leading optional
# positionally, so it stays poly, and the fallthrough handed it the keyword
# hash -- which the declared keyword had already consumed, binding it twice
# (#3525). A companion call passing the positional immunises this, which is
# why the eleven shapes in kwarg_with_omitted_leading_optional.rb could not
# fail: they share a file with calls that rescue them. One shape per file.
class A
  def self.f(a = nil, k: :default)
    "a=#{a.inspect} k=#{k.inspect}"
  end
end
puts A.f(k: 1)
