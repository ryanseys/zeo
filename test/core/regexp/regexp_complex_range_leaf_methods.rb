p Regexp.escape("a.b*c")
p Regexp.quote("1+1")
p(/abc/i.options)
p(/abc/m.options)
p(/abc/.options)
p Complex.rect(3, 4)
p Complex.rectangular(3)
p((1..10).bsearch { |x| x >= 4 })
p((1..100).bsearch { |x| x >= 40 })
p((1..10).bsearch { |x| x >= 40 })
__END__
"a\\.b\\*c"
"1\\+1"
1
4
0
(3+4i)
(3+0i)
4
40
nil
