# `io/console` and `io/nonblock` are require-gated extensions in ruby. zeo
# implements them natively, so their rows sit in `IO`'s own table -- one
# class owns one table -- and used to be answerable before the require.
#
# That is observable beyond reflection: `respond_to?(:getch)` is how a
# library decides whether the console extension is there at all, and a
# `NoMethodError` is what it expects when it is not.
#
# `IO#to_s` is the same list's other half in the opposite direction: ruby
# inherits it from `Kernel` and zeo declared it as IO's own.
p IO.instance_methods(false).include?(:getch)
p IO.instance_methods(false).include?(:nonblock)
p IO.instance_methods(false).include?(:to_s)
p IO.instance_method(:to_s).owner.to_s
p STDOUT.respond_to?(:winsize)
p STDOUT.respond_to?(:cooked)
p STDOUT.respond_to?(:nonblock)
p IO.respond_to?(:console)
p IO.singleton_methods(false).include?(:console)
begin
  STDOUT.getch
rescue NoMethodError => e
  p e.message
end
