# Each reopen's body is ONE statement, which is the shape that used to break:
# the guard was lifted out of the class-body function and asked of `Gm`.
module Gm
  require_relative "plat"

  class Plat
    unless respond_to?(:generic)
      WIDTH = 2
    end
  end

  class Plat
    if respond_to?(:generic)
      HEIGHT = 3
    end
  end

  class Plat
    unless const_defined?(:WIDTH)
      DEPTH = 4
    end
  end
end
