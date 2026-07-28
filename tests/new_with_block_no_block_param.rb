# `Klass.new { ... }` where `initialize` declares no block parameter. Ruby runs
# the block through `Proc.new`/`block_given?` and otherwise ignores it; zeo's
# static `.new` path passes it as a second argument to an `initialize` whose
# generated signature has no block slot, so the generated Rust fails to compile.
class Bag
  def initialize(**kw)
    @kw = kw
  end

  def show = @kw.inspect
end

p Bag.new(a: 1) { :ignored }.show
