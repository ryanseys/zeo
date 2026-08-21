# A reopen of `Array#each` makes the splice illegal: `iter_inline_ok_for`
# answers false and the site's other arm -- an ordinary block send -- is
# what finds the new definition. Without that guard a fused site would
# keep walking the array and never call the method at all.
class Array
  def each(&b) = "REDEFINED"
end
a = [1, 2, 3]
p a.each { |x| x }
