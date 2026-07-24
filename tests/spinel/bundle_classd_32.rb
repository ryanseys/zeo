# Bundled tests:
#   - proc_array_push
#   - proc_body_nested_block

# === proc_array_push ===
# Ensure that pushing a Proc into an ivar array (int_array default
# from []) promotes the array to poly_array so .each { |b| b.call }
# actually dispatches the proc. Without the fix, .call on the
# iteration variable is a no-op because b stays typed as mrb_int.
class T_proc_array_push_Runner
  def initialize
    @procs = []
  end

  def add(&block)
    @procs.push(block)
  end

  def call_all
    @procs.each do |p|
      p.call
    end
  end
end

r = T_proc_array_push_Runner.new
r.add { puts "first" }
r.add { puts "second" }
puts "before"
r.call_all
puts "after"

# === proc_body_nested_block ===
class T_proc_body_nested_block_Defer
  def initialize
    @blocks = []
  end

  def add(&blk)
    @blocks.push(blk)
  end

  def call_all
    @blocks.each do |b|
      b.call
    end
  end
end

class T_proc_body_nested_block_Runner
  def self.with_deferred(&outer)
    d = T_proc_body_nested_block_Defer.new
    outer.call(d)
    d.call_all
  end
end

T_proc_body_nested_block_Runner.with_deferred do |d|
  d.add { puts "first" }
  d.add { puts "second" }
  puts "mid"
end

