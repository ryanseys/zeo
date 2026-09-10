# shareable_constant_value: literal
# ShareableConstantNode — the `# shareable_constant_value:` magic comment.
#
# The pragma marks constants for Ractor-shareable initialization. The
# constant write it wraps runs normally either way, which is what this
# checks.

FOO = [1, 2, 3]
puts FOO.length
puts FOO.first
__END__
3
1
