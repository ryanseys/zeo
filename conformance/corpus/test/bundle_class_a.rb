# Bundled tests:
#   - alias_method
#   - and_node_poly_operand
#   - array_3d_nested

# === alias_method ===
# AliasMethodNode -- `alias greet hello` inside a class body.
#
# CRuby snapshots the method at alias time -- if `hello` is later
# redefined, `greet` still calls the original. In Spinel's AOT model
# methods are static C functions; we register a compile-time
# synonym so dispatch on `.greet` routes to the same C function as
# `.hello`. Out of scope: alias inside a method body (CRuby allows
# but the semantics differ); alias of an inherited method.

class T_alias_method_Greeter
  def hello = "hi from hello"
  alias greet hello
end

g = T_alias_method_Greeter.new
puts g.hello   # hi from hello
puts g.greet   # hi from hello

# Two aliases of the same method.
class T_alias_method_Counter
  def value = 42
  alias get value
  alias read value
end
c = T_alias_method_Counter.new
puts c.value   # 42
puts c.get     # 42
puts c.read    # 42

# === and_node_poly_operand ===
# `a && b` codegen used to lower to raw C `a && b` regardless of
# operand type. When either operand is `sp_RbVal` (poly), C rejects
# `&&` between a struct and an int. Wrap poly operands with
# `sp_poly_truthy`.

class T_and_node_poly_operand_Holder
  def initialize
    @poly = "x"   # string first…
    @poly = 42    # …then int — slot widens to poly
    @counter = 0
  end

  attr_reader :poly, :counter

  def chain
    if @poly && @counter == 0
      @counter = 1
    end
    @counter
  end
end

h = T_and_node_poly_operand_Holder.new
puts h.chain    # 1

# === array_3d_nested ===
# Deep-nested array literals (3D and beyond). Spinel doesn't
# have a typed `<X>_ptr_array_ptr_array` slot — the second-level
# `[[1,2,3],[4,5,6]]` infers as `int_array_ptr_array` and the
# outer `[[[...]],[[...]]]` would naively box each element via
# `sp_box_ptr_array`, which erases the elem-type info and
# leaves the dispatch returning an unknown obj at the next `[]`
# read.
#
# Fix: when an array literal element is itself a typed
# ptr_array (or already poly_array) and the outer is
# poly_array, recompile the inner literal as poly_array so
# every level boxes via `sp_box_poly_array` / `sp_box_int_array`
# / etc. — the cls_id chain stays tagged and the poly-builtin
# dispatch recurses correctly through `arr[i][j][k]...`.
#
# The recursion in `compile_array_literal_as_poly` makes the
# fix dimension-agnostic — 3D, 4D, 5D, ... all work.
#
# Without the fix `read` returned 0 for every index (sp_box_nil
# fallback). With the fix it returns the right ints.

class T_array_3d_nested_H3
  def initialize
    @t = [[[1, 2, 3], [4, 5, 6]], [[7, 8, 9], [10, 11, 12]]]
  end
  def read(i, j, k); @t[i][j][k]; end
end

h3 = T_array_3d_nested_H3.new
puts h3.read(0, 0, 0)   # 1
puts h3.read(0, 1, 1)   # 5
puts h3.read(1, 0, 2)   # 9
puts h3.read(1, 1, 2)   # 12

# 4D — recursion handles arbitrary depth.
class T_array_3d_nested_H4
  def initialize
    @t = [[[[1, 2], [3, 4]], [[5, 6], [7, 8]]],
          [[[9, 10], [11, 12]], [[13, 14], [15, 16]]]]
  end
  def read(i, j, k, l); @t[i][j][k][l]; end
end

h4 = T_array_3d_nested_H4.new
puts h4.read(0, 0, 0, 0)   # 1
puts h4.read(0, 1, 0, 1)   # 6
puts h4.read(1, 0, 1, 0)   # 11
puts h4.read(1, 1, 1, 1)   # 16

