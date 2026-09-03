# Array#join flattens nested arrays recursively with the same separator.
p [1, [2, 3], 4].join("-")
p [[1, 2], [3, 4]].join(",")
p [1, [2, [3, 4]]].join
p [[1], "a", [2, [3]]].join("|")
p [1, 2, 3].join("-")
__END__
"1-2-3-4"
"1,2,3,4"
"1234"
"1|a|2|3"
"1-2-3"
