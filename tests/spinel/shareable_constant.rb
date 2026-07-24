# shareable_constant_value: literal
# ShareableConstantNode — the `# shareable_constant_value:` magic comment.
#
# CRuby uses this to mark constants for Ractor-shareable initialization.
# Spinel has no Ractor runtime, so the shareability state is a no-op
# and the wrapped constant write executes normally.

FOO = [1, 2, 3]
puts FOO.length
puts FOO.first
