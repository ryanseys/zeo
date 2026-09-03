p(/(?<a>x)|(?<a>y)/.names)
p("y".match(/(?<a>x)|(?<a>y)/).names)
p(/(?<a>x)|(?<a>y)/.named_captures)
p("y".match(/(?<a>x)|(?<a>y)/)[:a])
p(/(?<a>x)(?<b>y)/.names)
__END__
["a"]
["a"]
{"a" => [1, 2]}
"y"
["a", "b"]
