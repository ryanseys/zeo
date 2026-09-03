# `C.prepend(M)` on a BUILTIN is a compile-time ancestry edit, and `super` from
# M's body must resume at C's NATIVE row. Two things were missing: a reopened
# builtin registers its whole FLATTENED table as value methods, so the `super`
# probe found M's own copy sitting on C and recursed forever; and a BOOTSTRAP
# exception class registers once in the runtime, so its chain never learned
# about the prepend at all.

module Loud
  def upcase = "<" + super + ">"
end
String.prepend(Loud)
p "ab".upcase
p String.ancestors.first(2).map(&:to_s)
p "ab".method(:upcase).owner.to_s
# Everything else on String is untouched.
p "ab".downcase
p "AB".swapcase

module Twice
  def size = super * 2
end
# Two prepends layer, most recent closest. Both written before the first read:
# WHEN an edit takes effect is the separate question `a_later_prepend_reaches_back`
# pins.
module Thrice
  def size = super + 1
end
Array.prepend(Twice)
Array.prepend(Thrice)
p [1, 2, 3].size
p [1, 2, 3].length
p Array.ancestors.first(3).map(&:to_s)

# A prepend on a BOOTSTRAP exception class.
module Wrap
  def message = "[" + super + "]"
end
StandardError.prepend(Wrap)
p StandardError.ancestors.first(2).map(&:to_s)
p StandardError.new("boom").message
# ... and it reaches every descendant, built-in and user alike.
p ArgumentError.new("bad").message
class MyErr < StandardError; end
p MyErr.ancestors.first(3).map(&:to_s)
p MyErr.new("mine").message
p MyErr.new("mine").to_s

# `include` on a builtin still loses to the class's own rows.
module Quiet
  def upcase = :quiet
end
class Symbol
  include Quiet
end
p :ab.upcase
p Symbol.ancestors.include?(Quiet)
__END__
"<AB>"
["Loud", "String"]
"Loud"
"ab"
"ab"
7
3
["Thrice", "Twice", "Array"]
["Wrap", "StandardError"]
"[boom]"
"[bad]"
["MyErr", "Wrap", "StandardError"]
"[mine]"
"mine"
:AB
true
