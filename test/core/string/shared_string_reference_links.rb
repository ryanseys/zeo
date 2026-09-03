# The same String written twice is a `@`-link the second time, so a
# mutation through one alias is visible through the other after load.

s = "ab"
g = Marshal.load(Marshal.dump([s, s]))
g[0] << "z"
p g[1]
__END__
"abz"
