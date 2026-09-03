# CRuby reports every offending keyword (singular/plural) with no
# `(in 'method')` suffix, for both direct calls and **hash forwarding.

def take(a:); a; end
def two(a:, b:); [a, b]; end
def rest(a:, **opts); [a, opts]; end
def try
  yield
rescue ArgumentError => e
  puts e.message
end
try { take(**{a: 1, b: 2}) }
try { two(**{a: 1, b: 2, c: 3, d: 4}) }
try { two(**{a: 1}) }
p rest(a: 1, x: 9, y: 8)
__END__
unknown keyword: :b
unknown keywords: :c, :d
missing keyword: :b
[1, {x: 9, y: 8}]
