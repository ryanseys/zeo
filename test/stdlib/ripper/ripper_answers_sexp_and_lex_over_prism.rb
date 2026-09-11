# `require "ripper"` defines Ripper. Its sexp, lex and tokenize answer the
# shapes rspec reads when it prints the source line of a failing example.
require "ripper"
p defined?(Ripper)
p Ripper.sexp("def foo; end")
p Ripper.sexp("expect(1).to eq(2)")
p Ripper.sexp("raise \"still broken\"")
p Ripper.lex("a = 1")
p Ripper.tokenize("x.y(1)")
p Ripper.sexp_raw("1 + 2")
p Ripper.sexp("def (")

# Ripper converts its source with `to_str`, as rspec's EncodedString needs;
# prism's own parser takes only a String.
src = Object.new
def src.to_str = "a = 1"
p Ripper.sexp(src)
p Ripper.lex(src).size
require "prism"
p((Prism.parse(src) rescue $!))
p((Prism.lex(1) rescue $!))
p((Ripper.sexp(nil) rescue $!))
begin
  require "ripper"
  puts "loaded"
rescue LoadError
  puts "no ripper"
end
__END__
"constant"
[:program, [[:def, [:@ident, "foo", [1, 4]], [:params, nil, nil, nil, nil, nil, nil, nil], [:bodystmt, [[:void_stmt]], nil, nil, nil]]]]
[:program, [[:command_call, [:method_add_arg, [:fcall, [:@ident, "expect", [1, 0]]], [:arg_paren, [:args_add_block, [[:@int, "1", [1, 7]]], false]]], [:@period, ".", [1, 9]], [:@ident, "to", [1, 10]], [:args_add_block, [[:method_add_arg, [:fcall, [:@ident, "eq", [1, 13]]], [:arg_paren, [:args_add_block, [[:@int, "2", [1, 16]]], false]]]], false]]]]
[:program, [[:command, [:@ident, "raise", [1, 0]], [:args_add_block, [[:string_literal, [:string_content, [:@tstring_content, "still broken", [1, 7]]]]], false]]]]
[[[1, 0], :on_ident, "a", CMDARG], [[1, 1], :on_sp, " ", CMDARG], [[1, 2], :on_op, "=", BEG], [[1, 3], :on_sp, " ", BEG], [[1, 4], :on_int, "1", END]]
["x", ".", "y", "(", "1", ")"]
[:program, [:stmts_add, [:stmts_new], [:binary, [:@int, "1", [1, 0]], :+, [:@int, "2", [1, 4]]]]]
nil
[:program, [[:assign, [:var_field, [:@ident, "a", [1, 0]]], [:@int, "1", [1, 4]]]]]
5
#<TypeError: wrong argument type Object (expected String)>
#<TypeError: wrong argument type Integer (expected String)>
#<TypeError: no implicit conversion of nil into String>
loaded
