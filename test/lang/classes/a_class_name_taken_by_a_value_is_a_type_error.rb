# `class X`/`module X` over a constant that is not a class (or is the other
# kind) raises TypeError before anything is minted. Ruby's message has a
# second line naming where the constant was set, with both fields empty for
# a builtin. A restated superclass must match, and an alias of a class
# reopens it. The same holds when the definition runs inside `eval`.
def t(label)
  yield
  puts "#{label}: no error"
rescue TypeError => e
  puts "#{label}: #{e.message.inspect}"
end

TOP = 1
t("static class") { class TOP; end }
t("static module") { module TOP; end }
class Foo; end
t("static kind") { module Foo; end }

t("eval class") { eval("class TOP; end") }
t("eval module") { eval("module TOP; end") }
Object.const_set(:SET, 2)
t("const_set") { eval("class SET; end") }
module Box1; INNER = 3; end
t("nested") { eval("class Box1::INNER; end") }
t("module over class") { eval("module String; end") }
t("class over module") { eval("class Kernel; end") }
t("user kind") { eval("module Foo; end") }
Alias = String
t("alias reopen") { eval("class Alias; end") }
t("superclass mismatch") { eval("class String < Integer; end") }
t("superclass restated") { eval("class String < Object; end") }
__END__
static class: "TOP is not a class\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:13: previous definition of TOP was here"
static module: "TOP is not a module\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:13: previous definition of TOP was here"
static kind: "Foo is not a module\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:16: previous definition of Foo was here"
eval class: "TOP is not a class\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:13: previous definition of TOP was here"
eval module: "TOP is not a module\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:13: previous definition of TOP was here"
const_set: "SET is not a class\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:21: previous definition of SET was here"
nested: "INNER is not a class\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:23: previous definition of INNER was here"
module over class: "String is not a module\n:: previous definition of String was here"
class over module: "Kernel is not a class\n:: previous definition of Kernel was here"
user kind: "Foo is not a module\nlang/classes/a_class_name_taken_by_a_value_is_a_type_error.rb:16: previous definition of Foo was here"
alias reopen: no error
superclass mismatch: "superclass mismatch for class String"
superclass restated: no error
