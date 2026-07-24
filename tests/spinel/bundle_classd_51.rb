# Bundled tests:
#   - value_type_recycled_field_clear
#   - volatile_discarded_at_call
#   - while_cond_side_effect
#   - while_condition_recomputed
#   - widen_int_obj_to_poly

# === value_type_recycled_field_clear ===
class T_value_type_recycled_field_clear_NameTag
  attr_reader :text

  def initialize(text)
    @text = text
  end
end

class T_value_type_recycled_field_clear_Holder
  attr_reader :tag, :junk

  def initialize(tag)
    @junk = []
    @tag = tag
  end
end

i = 0
while i < 3000
  T_value_type_recycled_field_clear_Holder.new(T_value_type_recycled_field_clear_NameTag.new("old" + i.to_s))
  i = i + 1
end

junk = []
i = 0
while i < 5000
  junk << "garbage" + i.to_s
  i = i + 1
end

holder = nil
i = 0
while i < 3000
  holder = T_value_type_recycled_field_clear_Holder.new(T_value_type_recycled_field_clear_NameTag.new("new" + i.to_s))
  i = i + 1
end

puts holder.tag.text

# === volatile_discarded_at_call ===
# #487. Top-level begin/rescue sets @needs_setjmp = 1, which emits
# every main() local as `volatile T *`. Passing such a local to a
# function expecting plain `T *` triggers gcc's
# -Wdiscarded-qualifiers / clang's
# -Wincompatible-pointer-types-discards-qualifiers. The program
# still runs correctly (volatile is strictly stronger than what the
# callee asks for); the fix casts away the qualifier at the call
# site so clean builds under -Wall stay green.

class T_volatile_discarded_at_call_M
  def self.read(env)
    env["k"]
  end
end

env = { "k" => "v" }
begin
  puts T_volatile_discarded_at_call_M.read(env)
rescue
  puts "rescued"
end

# === while_cond_side_effect ===
# #500. `while (n = gets.to_i) > 0` looped forever because codegen
# emitted `_t1 = sp_gets(); SP_GC_ROOT(_t1);` BEFORE the while line,
# so every iteration re-read the same captured first line. Fix:
# compile the predicate into a scratch buffer and replay its
# emits inside `while (1) { ...; if (!cond) break; ... }` so any
# transient temp/root tied to the receiver call lives per-iter.
#
# Test uses an instance method returning a String — `.to_i` then
# routes through compile_expr_gc_rooted's hoist for the heap-
# allocated receiver. Stdin gets isn't reachable from the test
# harness, so we drive the same code path via an iterator class.

class T_while_cond_side_effect_Source
  def initialize
    @data = ["5", "4", "3", "0"]
    @idx = 0
  end

  def next_line
    s = @data[@idx]
    @idx += 1
    s
  end
end

src = T_while_cond_side_effect_Source.new
count = 0
while (n = src.next_line.to_i) > 0
  count += 1
end
puts count

# === while_condition_recomputed ===
# A while condition that depends on a mutated receiver must be evaluated on
# every iteration.

items = [1, 2, 3]
sum = 0

while items.length > 0
  item = items.pop
  sum += item
end

puts sum
puts items.length

items2 = [4, 5, 6]
sum2 = 0

while items2.length > 0
  sum2 += items2.pop
end

puts sum2
puts items2.length

items3 = [7, 8, 9]
packed = 0

while items3.length != 0
  packed = packed * 10 + items3.shift
end

puts packed
puts items3.length

class T_while_condition_recomputed_IvarDrain
  def initialize
    @items = [10, 20, 30]
  end

  def drain
    sum = 0
    while @items.length > 0
      sum += @items.pop
    end
    sum
  end

  def remaining
    @items.length
  end
end

drain = T_while_condition_recomputed_IvarDrain.new
puts drain.drain
puts drain.remaining

# === widen_int_obj_to_poly ===
# When an ivar's first observed write is a definite int / nil
# literal and a later write assigns an obj-typed value, the slot
# was overwritten with the obj type — silently casting the prior
# int payload to a struct pointer. Subsequent dispatch through
# the slot then read garbage or computed pointer arithmetic.
#
# Widening to poly so the slot carries either case at runtime
# preserves the program's actual semantics: the dispatch path
# decides per cls_id at the call site.

class T_widen_int_obj_to_poly_Box
  def initialize(n)
    @n = n
    @arr = []
    @arr << n
  end
  attr_reader :n
end

class T_widen_int_obj_to_poly_Holder
  def initialize
    @poly = 10              # definite int first
    @poly = T_widen_int_obj_to_poly_Box.new(5)      # …then obj — slot widens to poly
  end
  attr_reader :poly
end

# poly recv → T_widen_int_obj_to_poly_Box#n via cls_id == T_widen_int_obj_to_poly_Box dispatch
puts T_widen_int_obj_to_poly_Holder.new.poly.n      # 5

