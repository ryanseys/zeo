# `Method#inspect` for methods zeo did NOT compile from Ruby source. The
# compiled-method shapes all match (see `tests/method_inspect_shapes.rb`);
# these four are what is left, and each needs something structural rather
# than a fix to the formatter.
#
#   * a method CRuby itself wrote in Ruby reports a location
#     (`<internal:numeric>:288`); zeo's equivalents are native and unlocated;
#   * zeo's `Integer#zero?` lives on `Numeric`, so it prints the owner
#     qualifier `Integer(Numeric)#zero?`;
#   * a per-object singleton (`def obj.m(x)`) is a runtime-defined body here,
#     carrying an arity but no parameter NAMES;
#   * CRuby collapses a fully anonymous forwarding signature to `(...)`.

def shape(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">")

# A builtin whose owner zeo places differently, and one CRuby wrote in Ruby.
p 1.method(:zero?).inspect
p 1.method(:integer?).inspect

# A per-object singleton knows its arity but not its parameter names.
class Widget; end
w = Widget.new
def w.only(x, y = 1) = x
p w.method(:only).arity
p shape(w.method(:only)).sub(/#<Widget:0x[0-9a-f]+>/, "OBJ")

# Anonymous forwarding parameters.
class Sig
  def anon(*, **, &) = 1
  def partly(x, *, **, &) = x
end
p shape(Sig.new.method(:anon))
p shape(Sig.new.method(:partly))
p Sig.instance_method(:anon).parameters
