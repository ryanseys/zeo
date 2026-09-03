module M
  def helper; end
end
class Base
  def pub; end
  private
  def priv; end
end
class Sub < Base
  include M
  def own; end
  def pub; end          # override
  def self.factory; end
end
puts Sub.instance_methods(false).sort.inspect   # [:own, :pub]
puts Sub.instance_methods.include?(:helper)      # true (module)
puts Sub.instance_methods.include?(:pub)         # true
puts Sub.instance_methods.include?(:priv)        # false (private)
puts Base.private_instance_methods(false).inspect # [:priv]
puts M.instance_methods.inspect                  # [:helper]
puts Sub.singleton_methods.inspect               # [:factory]
o = Sub.new
puts o.methods.include?(:own)                    # true
puts o.methods.include?(:factory)                # false
puts o.private_methods.include?(:priv)           # true
puts "s".methods.include?(:upcase)               # true
__END__
[:own, :pub]
true
true
false
[:priv]
[:helper]
[:factory]
true
false
true
true
