# A snippet that does not parse raises a `SyntaxError` whose message is the
# program's to read, so it is prism's whole report -- header, the offending
# line with its context, and one caret row per diagnostic -- exactly as
# CRuby builds it.

def report(src, file = nil, line = nil)
  eval(src, nil, *[file, line].compact)
rescue SyntaxError => e
  puts "----- #{src.inspect}"
  puts e.message
end

report("1 +", "F.rb", 7)
report("foo(", "F.rb", 7)
report("  x ]", "F.rb", 7)
report("a = 1\na = 1\nb +", "F.rb", 4)
report("\n" * 120 + "1 +", "F.rb", 3)
report("x = 'ありがとう' +\n", "F.rb", 3)

# With no file argument the rows name where the eval was written.
report("1 +")

# A snippet that does not parse is a SyntaxError, never a compiler refusal.
begin
  eval([":ok", "1 +"].last)
rescue SyntaxError => e
  puts e.class
end
__END__
----- "1 +"
F.rb:7: syntax errors found
> 7 | 1 +
    |    ^ unexpected end-of-input, assuming it is closing the parent top level context
    |    ^ unexpected end-of-input; expected an expression after the operator
----- "foo("
F.rb:7: syntax error found
> 7 | foo(
    |     ^ unexpected end-of-input; expected a `)` to close the arguments
----- "  x ]"
F.rb:7: syntax errors found
> 7 |   x ]
    |     ^ unexpected ']', ignoring it
    |     ^ unexpected ']', expecting end-of-input
----- "a = 1\na = 1\nb +"
F.rb:6: syntax errors found
  4 | a = 1
  5 | a = 1
> 6 | b +
    |    ^ unexpected end-of-input, assuming it is closing the parent top level context
    |    ^ unexpected end-of-input; expected an expression after the operator
----- "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n1 +"
F.rb:123: syntax errors found
  121 | 
  122 | 
> 123 | 1 +
      |    ^ unexpected end-of-input, assuming it is closing the parent top level context
      |    ^ unexpected end-of-input; expected an expression after the operator
----- "x = 'ありがとう' +\n"
F.rb:3: syntax errors found
> 3 | x = 'ありがとう' +
    |              ^ unexpected end-of-input, assuming it is closing the parent top level context
    |              ^ unexpected end-of-input; expected an expression after the operator
----- "1 +"
(eval at core/rubyvm/an_eval_syntax_error_carries_prisms_report.rb:7):1: syntax errors found
> 1 | 1 +
    |    ^ unexpected end-of-input, assuming it is closing the parent top level context
    |    ^ unexpected end-of-input; expected an expression after the operator
SyntaxError
