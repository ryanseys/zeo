# Ruby treats every byte above ASCII as an identifier byte, so this ivar is
# legal (fastimage ships one). Rust identifiers take XID characters only, and
# the name reached `Ident::new`, which panics rather than reporting anything.

@width, @height´ = nil
@height´ = 7
p [@width, @height´]

# A Rust KEYWORD as a class-level ivar takes the raw-identifier path, whose
# `r#type` spelling then had to survive being upper-cased into a static's name.
class Shape
  def self.type = @type
  def self.type=(v)
    @type = v
  end
end
Shape.type = "circle"
p Shape.type
__END__
[nil, 7]
"circle"
