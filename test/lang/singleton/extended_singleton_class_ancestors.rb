# The singleton class of a plain object renders as
# #<Class:#<Object:0xADDR>>; zeo renders it as the singleton of the object's
# CLASS (#<Class:Object>) once the object has been extended, losing the
# per-object identity in ancestors output.
mod = Module.new do
  def modded
    "modded"
  end
end

target = Object.new
target.extend(mod)
puts target.modded
puts target.singleton_class.ancestors.first(2).inspect
puts Object.new.singleton_class.inspect
__END__
modded
[#<Class:#<Object:0xADDR>>, #<Module:0xADDR>]
#<Class:#<Object:0xADDR>>
