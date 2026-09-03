# A method parameter is always statically Poly -- the universal
# `RubyValue::freeze_value`/`is_frozen` path, not the type-gated ones.

class Once
  def run(x)
    x.freeze
    puts x.frozen?
  end
end
Once.new.run([1])
Once.new.run(42)
__END__
true
true
