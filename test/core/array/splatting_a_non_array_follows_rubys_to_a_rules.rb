# Splatting a non-Array is ordinary Ruby, not an error -- it used to
# PANIC ("expected an Array to splat"), taking down `a, b = *1`.
# `[*"str"]` is the case worth pinning: it looks like it should split
# into characters and doesn't, because String has no `to_a`. Probing
# respond_to? rather than special-casing types gets that right, and
# gets a user class with its own to_a right too.

p [*1]
p [*nil]
p [*[1, 2]]
p [*{a: 1}]
p [*"str"]
p [*(1..3)]
class HasToA; def to_a; [7, 8]; end; end
p [*HasToA.new]
class NoToA; end
p([*NoToA.new].size)
p [*:sym]
a, b = *1
p [a, b]
c, d = *nil
p [c, d]
__END__
[1]
[]
[1, 2]
[[:a, 1]]
["str"]
[1, 2, 3]
[7, 8]
1
[:sym]
[1, nil]
[nil, nil]
