# A regex held in a local (notably an interpolated /.../ built at runtime)
# must dispatch gsub/sub/split to the compiled-pattern overloads, not the
# string-pattern ones.
re = /#{"(.)" * 3}/
p "ABCabc".gsub(re, '\1')
p "ABCabc".sub(re, '<\1>')
comma = /#{","}/
p "a,b,c".split(comma)
p "a,b,c,d".split(comma, 2)
ws = /\s+/
p "x  y   z".split(ws)
vowel = /[aeiou]/
p "hello world".gsub(vowel, "*")
