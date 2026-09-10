# Two classes each holding @key keep their own; the second's `||=` default is not the first's value.
class Foo
  def initialize
    @key = :enabled
  end
end
class Crash
  def key = @key ||= :url
end
Foo.new
raise unless Crash.new.key == :url
puts "ok"
__END__
ok
