def target(*a, **k) = [a, k]
ruby2_keywords def fwd(*a) = target(*a)
p fwd(1, k: 2)
__END__
[[1], {k: 2}]
