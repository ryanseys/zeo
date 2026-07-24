# ConstantPath access — covers ARGV (top-level builtin), built-in
# constructors (::Array.new etc.), nested const reads (A::B::C),
# class-method dispatch (A::B.create), and the relative-vs-root
# scope distinction (RootNS::Mid::LEAF vs ::RootNS::Mid::LEAF).
# Merged from constant_path_{argv,builtin_new,class_method,
# nested_read,new}.rb. The class-method section's `A::B` (a class)
# would collide with the nested-read section's `A::B::C` (where
# A::B is a module), so the class-method section is namespaced as
# CmA::CmB to keep both shapes co-resident.

# ---- ARGV access via ConstantPath ----
# Runs at top-level scope so sp_argv is in scope (sp_argv is only
# declared in main()).
puts ARGV.length
puts ::ARGV.length
puts(ARGV[0] == nil)
puts(::ARGV[0] == nil)

# ---- Built-in constructors via absolute ConstantPath ----
require "stringio"

bia = ::Array.new(3, 2)
puts bia[0] + bia[2]

bih = ::Hash.new
bih["k"] = bia[1]
puts bih["k"]

bip = ::Proc.new { |x| x + 1 }
puts bip.call(5)

bis = ::StringIO.new("ab")
puts bis.getc

bif = ::Fiber.new { |x|
  ::Fiber.yield(x + 1)
  x + 2
}
puts bif.resume(4)
puts bif.resume(4)

bicur = ::Fiber.current
puts bicur.alive?

# ---- ConstantPath class-method dispatch (A::B.create) ----
module CmA
  class CmB
    def self.create(x)
      x + 1
    end
  end
end
puts CmA::CmB.create(41)
puts ::CmA::CmB.create(99)

# ---- Nested ConstantPath reads (A::B::C, M::C::X) ----
module A
  module B
    C = 7
  end
end
module M
  class C
    X = 11
  end
end
puts A::B::C
puts ::A::B::C
puts M::C::X
puts ::M::C::X

# Relative path prefers lexical scope; `::` forces root scope.
module RootNS
  module Mid
    LEAF = 31
  end
end
module Lex
  module RootNS
    module Mid
      LEAF = 47
    end
  end
  def self.pick_relative
    RootNS::Mid::LEAF
  end
  def self.pick_root
    ::RootNS::Mid::LEAF
  end
end
puts Lex.pick_relative
puts Lex.pick_root

# ---- Top-level constructor via absolute ConstantPath (::TopC.new) ----
# Renamed from `class C` to TopC to keep it distinct from M::C above.
class TopC
  def initialize
    puts "init"
  end
end
puts "start"
::TopC.new
