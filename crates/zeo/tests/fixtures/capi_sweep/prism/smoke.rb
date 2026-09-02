require "prism"

r = Prism.parse("1 + 2\n")
puts r.success?, r.value.class, r.value.statements.body.first.class
p Prism.lex("a = 1").value.map { |t, _| t.type }
p Prism.parse_comments("# hi\n1").map(&:slice)
bad = Prism.parse("def")
puts bad.success?, bad.errors.first.message
puts Prism::VERSION
