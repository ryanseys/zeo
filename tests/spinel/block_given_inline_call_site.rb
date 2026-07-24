# Issue #706: block_given? in a yield-using method, called from
# a site that either omits the block (zero-arg case emits
# `sp_f(NULL, NULL)`) or supplies a literal block (inline path
# correctly answers true).
def f
  puts block_given?
end
f
f { }
