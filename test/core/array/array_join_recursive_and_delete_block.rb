# join flattens nested arrays under the same separator; delete's not-found
# block supplies the answer.

p [1, [2, [3, 4]], 5].join("-")
p [[1], "a", [2, [3]]].join("|")
p ["a", "b"].delete("z") { "missing" }
p ["a", "b"].delete("a") { "missing" }
p [1, 2].delete(9)
__END__
"1-2-3-4-5"
"1|a|2|3"
"missing"
"a"
nil
