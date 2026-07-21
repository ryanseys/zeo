# inject/reduce with an arithmetic operator passed as a symbol-to-proc block
# (inject(&:+)). Previously emitted an empty accumulation loop with an
# undeclared accumulator. The positional-symbol forms are unchanged. The
# symbol-to-proc fold seeds with the first element (no-init-value form).
p [1, 2, 3].inject(&:+)
p [1, 2, 3, 4].inject(&:*)
p [10, 3, 2].inject(&:-)
p [1, 2, 3].inject(:+)
p [1, 2, 3].inject(10, :+)
p [100, 20, 3].inject(&:+)
p [2, 3, 4].inject(&:*)
