# `Binding`, `Encoding`, `Rational` and `MatchData` carry no allocator in
# CRuby: the class definition is legal -- it registers a real class, takes
# class methods, and answers `ancestors`/`superclass` -- but `.new` raises
# NoMethodError, and every instance the runtime hands out is of the builtin
# class itself. Gems subclass them to hang a namespace or a class method off
# the name, never to build one, so zeo registers the definition and lets
# `.new` raise exactly what CRuby raises.
class MyBinding < Binding
  def self.label = :binding
end

class MyEncoding < Encoding
  def self.label = :encoding
end

class MyRational < Rational; end
class MyMatchData < MatchData; end

p MyBinding.superclass
p MyEncoding.superclass
p MyRational.superclass
p MyMatchData.superclass
p MyBinding.label
p MyEncoding.label
p MyRational.ancestors.take(3)

[MyBinding, MyEncoding, MyRational, MyMatchData].each do |klass|
  begin
    klass.new
  rescue NoMethodError => e
    puts "#{klass}: #{e.class}"
  end
end

# The builtins themselves keep making their own instances, and those are NOT
# of the subclass.
p Rational(3, 4).class
p "abc".match(/b/).class
p Encoding::UTF_8.class
p binding.class
puts "still running"
