# Regression for the `String#rindex` nil-guard in
# `collect_class_with_prefix`. A `class Sub < Parent` written
# inside a single-level `module M` triggers the parent-resolution
# walk that strips trailing `_<segment>` from the module prefix.
# `mp = "M"` has no underscore, so `mp.rindex("_")` returns nil
# under CRuby (and -1 under spinel). The pre-fix `if idx < 0`
# crashed on the CRuby path with `NoMethodError: undefined
# method '<' for nil`.

module M
  class Sub < Object
    def hello; puts "hi"; end
  end
end
M::Sub.new.hello
