# Ruby labels the backtrace frame of a `class << self` body `singleton class`
# -- a fixed spelling, not a class name. zeo homes such a body on a surrogate
# class under the reserved name `#<Class:self>`, and printed THAT: an internal
# name, unwritable in ruby, reaching a user's backtrace.
class Config
  class << self
    [1].each { |n| define_method("m#{n}") { raise "boom" } }
  end
end

begin
  Config.m1
rescue RuntimeError => e
  puts e.backtrace.first
  puts e.message
end

# A `def self.x` beside it keeps naming the class, not the singleton.
class Config
  class << self
    def direct = raise("direct")
  end
end
begin
  Config.direct
rescue RuntimeError => e
  puts e.backtrace.first
end
__END__
lang/singleton/a_singleton_body_frame_is_spelled_singleton_class.rb:7:in 'block (2 levels) in singleton class'
boom
lang/singleton/a_singleton_body_frame_is_spelled_singleton_class.rb:21:in 'Config.direct'
