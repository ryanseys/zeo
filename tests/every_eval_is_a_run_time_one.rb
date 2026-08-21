# A literal `eval("...")` was once SPLICED: parsed at compile time and run
# as ordinary code in the enclosing scope. That was an optimization with a
# price, and every part of the price is visible from Ruby -- `__FILE__` and
# every backtrace row named the ENCLOSING file where CRuby names
# `(eval at f.rb:2)`, a magic comment written in the string was ignored,
# and the snippet shared the caller's storage outright where CRuby shares
# a Binding. The splice is gone: every `eval` is a run-time one, compiled
# by the same front end and cached by source.

def here(text)
  text.sub(__FILE__, "F")
end

def where
  eval("[__FILE__, __LINE__]")
end
p here(where.first)
p where.last

def raiser
  eval("raise 'boom'")
end
begin
  raiser
rescue => e
  puts e.backtrace.first(2).map { |l| here(l) }
end

p eval("# frozen_string_literal: true\n'abc'.frozen?")
p eval("'abc'.frozen?")

# A `BEGIN` block belongs to the snippet, not to the program's head.
eval("BEGIN { puts 'begin ran' }")
puts "after"

# The caller's locals are still shared -- through the Binding the site
# materializes, which is CRuby's own model.
x = 1
eval("x = x + 41")
p x
p eval("eval('x + 1', binding)")
