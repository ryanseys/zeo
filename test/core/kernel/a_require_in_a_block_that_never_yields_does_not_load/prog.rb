def self.never_yields
  :no_yield
end
never_yields do
  require_relative "dep"
end
p Object.const_defined?(:DEP)
p [1, 2].map { require_relative "dep" }
p DEP
