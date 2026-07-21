# A named-module_function method that no in-unit call site reaches
# (even via a chain of other uncalled module functions) is dead-code-
# eliminated, so its body — which may use unimplemented dynamic
# patterns — doesn't break compilation. A *called* module function
# still works. Regression for #1062.
module M
  # uncalled; body builds from a variable-held class (dynamic .new)
  def fail_with(*args)
    klass = args.shift
    e = klass.new(*args)
    e.set_backtrace([])
    e
  end
  module_function :fail_with

  # uncalled, but calls the above (chain) — both must be dead
  def boom(*args)
    fail_with(*args)
  end
  module_function :boom

  # CALLED — must stay live and work
  def greet(name)
    "hi " + name
  end
  module_function :greet
end
puts M.greet("world")
puts(2 + 3)
