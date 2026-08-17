# `===` on a boxed receiver dispatches on the receiver's runtime class the way
# CRuby does: a Range covers, a Regexp matches, a Class tests membership. It
# used to compile as plain equality, so all three answered false (#3963).
p [1..10].map { |r| r === 5 }
p [/b/].map  { |r| r === "abc" }
p [Integer].map { |c| c === 5 }
p [1..10].map { |r| r.===(5) }
p ["a".."e"].map { |r| r === "c" }
h = { a: 1..10 }
p h.map { |k, r| r === 5 }
p [1..10].map { |r| 5.then { |v| r === v } }
p [1..10].select { |r| r === 5 }
p [1..10, 20..30].map { |r| r === 5 }
p [1..5, /b/, Integer, "x"].map { |pat| pat === 3 }
p [1..5, /b/, Integer, "x"].map { |pat| pat === "abc" }
p [->(x) { x * 2 }].map { |f| f === 5 }

# The Regexp surface on a boxed receiver (#3961)
p [/b/].map { |re| re.source }
p [/b/].map { |re| re.match?("abc") }
p [/b/].map { |re| "abc".match?(re) }
p [/b/].map { |re| re.options }
p [/b/i].map { |re| re.casefold? }
p({ a: /b/ }.map { |k, re| re.source })
p [/b/].map { |re| re.match("abc")[0] }
p [/b/].map { |re| ("abc" =~ re) }
p [/b/].map { |re| (re =~ "abc") }
p [/z/].map { |re| ("abc" =~ re) }
p [/(?<x>b)/].map { |re| re.names }
