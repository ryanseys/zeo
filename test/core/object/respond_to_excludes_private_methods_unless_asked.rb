# The one-argument form answers false for a private method; the two-argument
# form answers true.
klass = Class.new do
  private

  def secret; end
end
p klass.new.respond_to?(:secret)
p klass.new.respond_to?(:secret, true)
__END__
false
true
