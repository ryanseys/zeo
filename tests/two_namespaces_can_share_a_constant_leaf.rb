# Two namespaces may each hold a constant of the same LEAF name, and neither
# may answer for the other. One row per shape, printed as `name<TAB>result`.
#
# zeo decides at LOWERING time whether a `class X < Super` is a compile-time
# class or a runtime `X = Class.new(Super)`, and that decision reads two
# whole-program tables: "is this constant assigned?" and "is this constant a
# class definition?". The second always asked from the CREF the site is
# written in; the first asked by BARE LEAF, over the whole program.
#
# So one namespace's `Path = Struct.new(...) do ... end` made every `class X <
# Path` anywhere look like a subclass of a runtime-minted class, and a plain
# `class Path` in another namespace was rewritten into a runtime REOPEN of a
# class that does not exist there -- `uninitialized constant Files::Path`,
# four lines into this file, for ordinary Ruby.
#
# Under `require "bundler"` the same collision was quieter and worse:
# `Bundler::Settings::Path` rewrote `class Git < Path` in
# `bundler/source/git.rb` to `Git = Class.new(Path)`, so
# `Bundler::Source::Git` existed TWICE -- once as a compile-time class
# nothing ever revealed, once as the runtime class in the constant table.
# `const_get(:Git)` answered the second and `Bundler::Source::Git` raised on
# the first.
#
# The fix is one rule: a scope-less write is keyed by its cref path, and the
# query is Ruby's own lexical search -- exactly what `record_class_def` and
# `class_defined_in_scope` already did for the class half. Two hand-rolled
# leaf-matching workarounds went away with it.
#
# Every row below is ordinary Ruby with an unambiguous answer.

def row(name)
  print name, "\t"
  begin
    puts(yield.inspect)
  rescue Exception => e
    puts "#{e.class}: #{e.message}"
  end
end

# --- 1. a runtime-minted constant beside a same-leaf class -------------------

module Settings
  Path = Struct.new(:a) do
    def kind = "struct"
  end
end

module Files
  class Path
    def kind = "class"
  end
  class Sub < Path
  end
end

row("settings_path_kind") { Settings::Path.new(1).kind }
row("files_path_kind") { Files::Path.new.kind }
row("files_sub_superclass") { Files::Sub.superclass.to_s }
row("files_sub_kind") { Files::Sub.new.kind }
row("files_sub_name") { Files::Sub.name }
# The class must be the SAME object by both routes -- the failure this probe
# was written for produced two.
row("files_path_identity") { Files::Path.equal?(Files.const_get(:Path)) }
row("files_sub_identity") { Files::Sub.equal?(Files.const_get(:Sub)) }
row("files_constants") { Files.constants.sort.inspect }

# --- 2. the collision in the other order ------------------------------------

module Early
  class Token
    def kind = "class"
  end
  class Tagged < Token
  end
end

module Late
  Token = Class.new do
    def kind = "runtime"
  end
end

row("early_tagged_superclass") { Early::Tagged.superclass.to_s }
row("early_tagged_kind") { Early::Tagged.new.kind }
row("late_token_kind") { Late::Token.new.kind }

# --- 3. a genuine runtime superclass still works ----------------------------

module Real
  Base = Class.new do
    def kind = "runtime-base"
  end
  class Derived < Base
    def extra = 7
  end
end

row("real_derived_superclass") { Real::Derived.superclass.equal?(Real::Base) }
row("real_derived_kind") { Real::Derived.new.kind }
row("real_derived_extra") { Real::Derived.new.extra }
row("real_derived_name") { Real::Derived.name }

# --- 4. a qualified superclass naming a runtime constant --------------------

module Outer
  module Inner
    Impl = Class.new do
      def kind = "inner-impl"
    end
  end
  class Uses < Inner::Impl
  end
end

row("outer_uses_superclass") { Outer::Uses.superclass.equal?(Outer::Inner::Impl) }
row("outer_uses_kind") { Outer::Uses.new.kind }

# --- 5. a bare top-level constant must not capture a nested class -----------

Marker = 7

module Holder
  class Marker
    def kind = "nested"
  end
  class UsesMarker < Marker
  end
end

row("toplevel_marker") { Marker }
row("holder_marker_kind") { Holder::Marker.new.kind }
row("holder_uses_superclass") { Holder::UsesMarker.superclass.to_s }

# --- 6. the same leaf three deep --------------------------------------------

module A1
  Value = Class.new { def where = "A1" }
end
module B1
  class Value
    def where = "B1"
  end
  class Uses < Value; end
end
module C1
  Value = 3
end

row("a1_value_where") { A1::Value.new.where }
row("b1_uses_where") { B1::Uses.new.where }
row("b1_uses_superclass") { B1::Uses.superclass.to_s }
row("c1_value") { C1::Value }

# --- 7. reopening: a bare `class X` where an unrelated X is runtime-minted --

module Mint
  Shape = Class.new { def kind = "minted" }
end

module Plain
  class Shape
    def one = 1
  end
  class Shape
    def two = 2
  end
end

row("mint_shape_kind") { Mint::Shape.new.kind }
row("plain_shape_one") { Plain::Shape.new.one }
row("plain_shape_two") { Plain::Shape.new.two }
row("plain_shape_ancestors") { Plain::Shape.ancestors.first.to_s }

# --- 8. a real reopen of a runtime-minted class still reopens ---------------

module Reopen
  Thing = Class.new { def one = 1 }
  class Thing
    def two = 2
  end
end

row("reopen_thing_one") { Reopen::Thing.new.one }
row("reopen_thing_two") { Reopen::Thing.new.two }
row("reopen_thing_count") { Reopen.constants.count { |c| c == :Thing } }

# --- 9. `defined?` and const_get must agree with the read -------------------

module Agree
  class Leaf; end
  class Branch < Leaf; end
end

row("agree_defined_leaf") { defined?(Agree::Leaf) }
row("agree_defined_branch") { defined?(Agree::Branch) }
row("agree_get_equals_read") { Agree.const_get(:Branch).equal?(Agree::Branch) }
row("agree_branch_instance") { Agree::Branch.new.is_a?(Agree::Leaf) }

# --- 10. a same-leaf module and class ---------------------------------------

module KindA
  Mixed = Module.new
end
module KindB
  class Mixed; end
  class UsesMixed < Mixed; end
end

row("kinda_mixed_class") { KindA::Mixed.class.to_s }
row("kindb_mixed_class") { KindB::Mixed.class.to_s }
row("kindb_uses_superclass") { KindB::UsesMixed.superclass.to_s }
