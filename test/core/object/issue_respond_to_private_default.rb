# Object#respond_to?(:name) (the one-argument form) should return false for
# a private method -- only the two-argument form (`respond_to?(:name,
# true)`) should include private methods. zeo returns true in both cases,
# not distinguishing the default `include_all` value.
klass = Class.new do
  private

  def secret; end
end
p klass.new.respond_to?(:secret)
p klass.new.respond_to?(:secret, true)
__END__
false
true
