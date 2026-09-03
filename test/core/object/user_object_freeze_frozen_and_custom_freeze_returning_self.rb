# The `freeze_user_object` scenario end to end: a bareword self-`freeze`
# in `initialize` sets real state, `frozen?` reads it back, mutation
# after freeze raises FrozenError, and a user `def freeze = (@log =
# "custom"; self)` (the `Seq`-tail-`self` shape whose codegen boxing was
# the E0308) returns self, runs its body, and -- since it overrides the
# real freeze -- leaves the object UNfrozen. (The FrozenError message's
# inspect tail `#<Sealed:0x.. @x=1>` is a separate default-inspect gap,
# so this asserts the message PREFIX, not the address/ivar detail.)

class Sealed
  attr_accessor :x
  def initialize
    @x = 1
    freeze
  end
end
o = Sealed.new
p o.frozen?
begin
  o.x = 2
rescue FrozenError => e
  puts "FrozenError: #{e.message.start_with?('can\'t modify frozen Sealed')}"
end
p o.x

class OwnFreeze
  attr_reader :log
  def initialize = (@log = "clean")
  def freeze = (@log = "custom"; self)
end
f = OwnFreeze.new
p f.freeze.equal?(f)
p f.log
p f.frozen?
__END__
true
FrozenError: true
1
true
"custom"
false
