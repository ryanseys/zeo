# Regression for the `String#rindex` nil-guard in
# `collect_class_with_prefix`. A `class Sub < Parent` written
# inside a single-level `module M` triggers the parent-resolution
# walk that strips trailing `_<segment>` from the module prefix.
# `mp = "M"` has no underscore, so `mp.rindex("_")` answers nil, and `if idx
# < 0` on nil raises NoMethodError. A miss is nil, not -1.

module M
  class Sub < Object
    def hello; puts "hi"; end
  end
end
M::Sub.new.hello
__END__
hi
