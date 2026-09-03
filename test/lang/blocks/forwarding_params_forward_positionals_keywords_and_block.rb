# `...` is real now (G3): it desugars to internal `*__fwd_rest,
# **__fwd_kw, &__fwd_blk` params referenced by the call-site `...`.

def target(a, b, mode: "m")
  r = yield if block_given?
  "#{a}/#{b}/#{mode}/#{r.inspect}"
end
def fwd(...)
  target(...)
end
puts fwd(1, 2, mode: "z") { "blk" }
__END__
1/2/z/"blk"
