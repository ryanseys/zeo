if ENV["ZEO_NOPE"]
  class Gated
    def x = 1
  end
end
puts defined?(Gated).inspect
puts Object.constants.include?(:Gated)
begin
  Object.const_get(:Gated)
rescue NameError => e
  puts "raised #{e.class}"
end
__END__
nil
false
raised NameError
