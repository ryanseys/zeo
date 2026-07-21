# Issue #304: module-dispatch ternary call sites
# (`Disp.adapter.method(args)` where Disp.adapter resolves to N
# candidate classes via the module-singleton-accessor table) used
# to leave the candidates' class-method params at their pre-widen
# narrow type even when the only caller passed a poly value. The
# emitted ternary would type-mismatch each arm.
#
# Two fixes: (1) module_sentinel now distinguishes class candidates
# (was returning 0 for class names looked up against the module
# table only — every ternary arm ended up testing `slot == 0` and
# the dispatch always picked the first candidate); (2) scan_new_calls
# walks the `Disp.adapter.update(@id)` chain shape and unifies the
# args' types into every candidate's `@cls_cmeth_ptypes`.
#
# Sister to #207 / #286 / #287 — same family of "widening must
# propagate across the dispatch path."

class A
  def self.update(id)
    id
  end
end

class B
  def self.update(id)
    id
  end
end

module Disp
  class << self
    attr_accessor :adapter
  end
end

class C
  def initialize
    @id = 0
  end
  def write_str(s); @id = s; end   # widens @id to poly per #247
  def save
    Disp.adapter.update(@id)         # ternary-dispatched: poly arg
  end
end

if ARGV.length > 0
  Disp.adapter = A
else
  Disp.adapter = B
end

c = C.new
c.write_str("hi")
c.save                              # no output expected; just compile
puts "ok"
