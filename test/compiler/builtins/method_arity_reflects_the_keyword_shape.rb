# A required keyword adds one fixed mandatory slot; an optional keyword or
# keyword-rest with NO required keyword makes the method variadic. An
# optional positional or rest is variadic; a block never affects arity.

def none; end
def two(a, b); end
def opt(x, y = 1); end
def splat(*xs); end
def post_splat(a, *b, c); end
def opt_kw(a, b: 1); end
def req_kw(a, b:); end
def mix_kw(a, b:, c: 1); end
def kw_rest(a, **kw); end
def req_kw_rest(a, b:, **kw); end
def with_blk(a, &blk); end
p [method(:none).arity, method(:two).arity, method(:opt).arity,
   method(:splat).arity, method(:post_splat).arity, method(:opt_kw).arity,
   method(:req_kw).arity, method(:mix_kw).arity, method(:kw_rest).arity,
   method(:req_kw_rest).arity, method(:with_blk).arity]
__END__
[0, 2, -2, -1, -3, -2, 2, 2, -2, 2, 1]
