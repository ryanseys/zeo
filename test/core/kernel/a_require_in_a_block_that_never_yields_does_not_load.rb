# A block body runs only when something yields to it, so a `require` written
# inside one is not a load this compile can perform ahead of time. rack's
# test helper is the shape: `separate_testing do require_relative "..." end`,
# where the non-SEPARATE definition of that method does not yield.

require_relative "a_require_in_a_block_that_never_yields_does_not_load/prog"
__END__
false
dep ran
[true, false]
:dep
