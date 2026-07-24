# `self.<acc>` / `self.<acc> = v` for a module-level singleton accessor
# (`class << self; attr_accessor`) must resolve to the same storage that
# `Mod.<acc>` uses. Before, only an explicit module-name receiver worked;
# a `self` receiver typed as int and the call fell through to the
# NoMethodError gate, so the write in a module body was lost (read nil).

# self.x = ... in the module body, runs at definition time
module M
  class << self; attr_accessor :x; end
  self.x = [2, 3]
end
p M.x

# self.x= and self.x inside a module class method
module N
  class << self; attr_accessor :val; end
  def self.setup; self.val = 41; end
  def self.bump; self.val = self.val + 1; end
end
N.setup
N.bump
p N.val

# Regression: explicit Mod.acc = still works alongside the self form
module P
  class << self; attr_accessor :data; end
  self.data = "in-body"
end
p P.data
P.data = "after"
p P.data
