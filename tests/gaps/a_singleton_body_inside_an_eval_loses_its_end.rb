# A `class << self` written inside a CLASS in a snippet loses its `end`.
#
# A class body inside an `eval` runs as one more `class_eval` of its own
# SOURCE TEXT, which the emitter recovers by slicing the snippet between the
# first and last body statement (`clif::stmt::eval_body_source`). That works
# because a body statement's span is its own source.
#
# `class << self` is not a statement: `lower::defs` SPLICES it into the
# enclosing class body -- an empty surrogate reopen carrying the `class <<
# self` keyword's span, then the retagged `def`s carrying theirs. So the
# slice runs from the `class << self` keyword to the last `def`, and the
# `end` that closes the singleton body is outside it. prism then reports a
# `SyntaxError` about an unterminated `class`.
#
# Two things this is NOT: it is not the `class << self` itself (a snippet
# whose TOP LEVEL is one works, and so does `class << obj`), and it is not
# the FFI-struct shape that used to share this refusal -- that one is
# closed, see `tests/an_ffi_struct_declared_inside_an_eval.rb`.
#
# The fix shape: in eval mode the splice should not happen. Keep the whole
# `class << self ... end` as ONE `ClassDef` under the reserved surrogate
# name, spanning the keyword to its `end`, and teach `eval_class_def` to
# open `self.singleton_class` for that name instead of minting a class
# called `#<Class:self>`. The run-time `class_eval` then does the retagging
# the compile-time splice does, because a `def` in a singleton class body
# already IS a class method there.
#
# Oracle: ruby answers the class method.
eval <<~SRC
  class SelfSing
    class << self
      def hi = "class method"
    end
  end
SRC
begin
  p SelfSing.hi
rescue Exception => e
  puts "#{e.class}: #{e.message.lines.first.to_s.strip[0, 60]}"
end
