# `defined?(yield)` at the top level makes zeo emit Rust that does not compile:
#
#     error[E0425]: cannot find value `__blk` in this scope
#
# Codegen lowers `defined?(yield)` to a test of the enclosing method's block
# parameter, `__blk`. At the top level there is no enclosing method and so no
# such binding, and nothing declines the lowering first. Ruby answers `nil`:
# there is no block here, which is exactly what `defined?` is being asked.
#
# A compile failure is worse than a wrong answer -- the whole program is
# rejected over an expression whose value is a constant `nil`.

p defined?(yield)

# Inside a method both answers are already right; only the top-level form fails.
def with_block = defined?(yield)
p with_block
p(with_block { })
__END__
nil
nil
"yield"
