# Reopening a class through a constant alias (`CONST = SomeClass` or
# `CONST = expr.class`) reopens that class, and a module included into a
# reopened built-in becomes dispatchable on the primitive. Regression
# for #1036 (the to_words gem's `INTEGER_KLASS = 1.class; class
# INTEGER_KLASS; include ToWords; end` shape).

module IntWords
  def wordy
    "wordy_" + self.to_s
  end
end

# Constant alias to a built-in (via `.class`) + module include.
INT_ALIAS = 1.class
class INT_ALIAS
  include IntWords
end
puts 42.wordy

module Greeter
  def greet
    "hi from greeter"
  end
end

# Constant alias to a user class + module include.
class Box
end
BOX_ALIAS = Box
class BOX_ALIAS
  include Greeter
end
puts Box.new.greet
