def self.never_yields
  :no_yield
end
require_relative "lib/pkg" if ENV["HOME"]
never_yields do
  require_relative "lib/pkg/target"
end
puts "before read"
p Pkg::Target.read
