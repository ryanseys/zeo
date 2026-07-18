# Runtime string `eval` (#97 stage 2). Each source is a runtime VALUE (a
# variable, not a string literal), so it runs through the eval VM -- a
# tree-walking interpreter over prism, linked into the runtime -- rather than
# the compile-time inline path a string LITERAL takes. Scope: a correct `self`,
# constants, and globals, but not the caller's own locals (a first-class
# `binding` is a later increment).

# Arithmetic honours precedence.
puts eval("1 + 2 * 3".dup)

# Method calls on evaluated receivers.
puts eval("[3, 1, 2].sort.inspect".dup)
puts eval("'Ruby'.upcase".dup)
puts eval("(-5).abs".dup)

# Locals defined and read within the SAME eval share one scope.
puts eval("a = 10; b = 20; a + b".dup)

# String interpolation inside the eval'd source.
puts eval('x = 5; "x squared is #{x * x}"'.dup)

# The source itself is assembled at runtime.
op = "*"
lhs = 6
rhs = 7
puts eval("#{lhs} #{op} #{rhs}")

# Control flow -- the value is the last expression evaluated.
puts eval("if 2 > 1 then 'yes' else 'no' end".dup)
puts eval("total = 0; i = 1; while i <= 5; total = total + i; i = i + 1; end; total".dup)

# Short-circuit operators return the operand, not a coerced boolean.
puts eval("nil || 'fallback'".dup)

# Constants resolve -- builtin classes, module constants, method calls on them.
puts eval("Integer".dup)
puts eval("Math::PI".dup).round(3)
puts eval("Math.sqrt(144)".dup)

# Collections.
puts eval("[1, 2, 3, 4].length".dup)
puts eval("{ a: 1, b: 2 }.length".dup)
puts eval("(1..5).to_a.inspect".dup)

# eval sees top-level globals and the main object's ivars.
$greeting = "hi"
puts eval("$greeting".dup)
@count = 41
puts eval("@count + 1".dup)

# A non-String argument is a TypeError, exactly as in CRuby.
begin
  eval(123)
rescue TypeError => e
  puts e.message
end

# A syntax error in the source is a catchable SyntaxError.
begin
  eval("1 +".dup)
rescue SyntaxError
  puts "caught SyntaxError"
end
