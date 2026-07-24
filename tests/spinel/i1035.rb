# `module_function :name` (argument form) promotes an already-defined
# instance method to a module (class) method callable as M.name.
module M
  def which(x)
    return x if x == "found"
    nil
  end
  module_function :which

  def exist?(x)
    !which(x).nil?
  end
  module_function :exist?
end

puts M.exist?("found")
puts M.exist?("missing")
puts M.which("found")
