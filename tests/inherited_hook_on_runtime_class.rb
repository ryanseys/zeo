# `Class.new(Base)` fires Base's `inherited` hook at creation -- before the
# body block runs and before any constant names the class (the hook sees
# `name == nil`), exactly as a literal `class Named < Base` does. minitest's
# whole Runnable registry is this hook, fired by every spec-DSL `describe`.
class Base
  def self.inherited(k)
    super
    p [:inherited, k.name]
  end
end
K = Class.new(Base)
class Named < Base; end
k2 = Class.new(Base)
p :done
