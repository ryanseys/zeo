# Issue #126 Stage 2: a module-level singleton accessor that's
# written with two or more distinct module/class names goes through
# a runtime sentinel switch instead of being constant-folded.
# `resolve_module_singleton_accessors` records the union of all
# constant RHSes; the write site emits `slot = SP_MOD_<X>;` (using
# the module's `module_sentinel` integer); the chain dispatch
# emits an if-cascade over the slot.

# 1. Two adapters reassigned — same shape as a Rails-style
#    runtime adapter swap.
module SqliteAdapter
  def self.name; "sqlite"; end
end

module PgAdapter
  def self.name; "postgres"; end
end

module ActiveRecord
  class << self
    attr_accessor :adapter
  end
end

ActiveRecord.adapter = SqliteAdapter
puts ActiveRecord.adapter.name      # sqlite

ActiveRecord.adapter = PgAdapter
puts ActiveRecord.adapter.name      # postgres

ActiveRecord.adapter = SqliteAdapter
puts ActiveRecord.adapter.name      # sqlite

# 2. Three candidates with conditional assignment driving which
#    sentinel ends up in the slot.
module A
  def self.tag; "A"; end
end
module B
  def self.tag; "B"; end
end
module C
  def self.tag; "C"; end
end

module Pick
  class << self
    attr_accessor :slot
  end
end

[1, 2, 3, 1].each do |n|
  if n == 1
    Pick.slot = A
  elsif n == 2
    Pick.slot = B
  else
    Pick.slot = C
  end
  puts Pick.slot.tag
end
# A B C A
