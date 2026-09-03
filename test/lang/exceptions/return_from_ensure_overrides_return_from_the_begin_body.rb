# Real, tricky Ruby semantic: a `return` inside `ensure` wins over a
# `return` already in flight from `begin`'s own body -- not just "ensure
# runs afterward", it actually REPLACES the method's return value.
# Falls out for free here: `ensure`'s own statements lower inline
# (see `clif::control::lower_begin`), so a `return` inside it
# exits the enclosing method directly, superseding the return value
# already in flight.

class M
  def m
    begin
      return "from begin"
    ensure
      return "from ensure"
    end
  end
end
puts M.new.m
__END__
from ensure
