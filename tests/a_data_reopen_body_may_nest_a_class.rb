# Reopening a `Data.define` constant with a bare `class` runs the body as a
# `class_eval` block, and every class-body statement but a local write has a
# spelling there -- a nested class included. The nested `class Spec` used to
# send the whole definition back to the static path, which minted a
# memberless `Target` over the Data class, so `Target.new(name:)` inside
# `Spec#fin!` raised "wrong number of arguments (given 1, expected 0)".
#
# Sow's `lib/sow/reap/target.rb` + `target/spec.rb` is this shape across two
# files, which `require_relative` below reproduces.

module Pkg
  Target = Data.define(:name)
  class Target
    class Spec
      def fin! = Pkg::Target.new(name: :t)
    end
    include Comparable
    def <=>(o) = name <=> o.name
  end
end
p Pkg::Target::Spec.new.fin!.name
p Pkg::Target.new(name: :b) > Pkg::Target.new(name: :a)
p Pkg::Target.ancestors.include?(Data)

require_relative "a_data_reopen_body_may_nest_a_class/target"

spec = Pkg3::Target::Spec.new(:t)
spec.build { |x| x + 1 }
t = spec.fin!
p t.name, t.call(41)
p Pkg3::Target.ancestors.include?(Data)
begin
  Pkg3::Target::Spec.new(:u).fin!
rescue ArgumentError => e
  p e.message
end
