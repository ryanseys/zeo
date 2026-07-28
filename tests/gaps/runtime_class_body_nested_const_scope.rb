# A `Class.new do ... end` body is an ordinary BLOCK, and a block opens no
# lexical scope for constants -- so `Inner = Class.new { ... }` written inside
# one lands at TOP level, not under `Outer`. zeo lowers the nested definition
# through its runtime-class desugar, which namespaces it under the enclosing
# class instead, so `Outer.const_defined?(:Inner, false)` answers true where
# ruby answers false. (Both agree that bare `Inner` resolves.)
Outer = Class.new do
  Inner = Class.new do
    def label = "inner"
  end
end

p Inner.new.label
p Outer.const_defined?(:Inner, false)
p Object.const_defined?(:Inner, false)
