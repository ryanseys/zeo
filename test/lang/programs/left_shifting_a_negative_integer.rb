# `-8 << 1` and `-1 << 4` keep the sign.
# (spinel issue #2870)
p(-8 << 1)
p(-1 << 4)
__END__
-16
-16
