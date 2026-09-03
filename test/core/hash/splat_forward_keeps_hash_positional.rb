def sink(a = 1, *rest, k: 2, **kw) = [a, rest, k, kw]

def capture(*args) = args
p capture(9, k: 3)

def forward(*args) = sink(*args)
p forward(9, k: 3)
__END__
[9, {k: 3}]
[9, [{k: 3}], 2, {}]
