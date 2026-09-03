def lazy
  require "ostruct"
  o = OpenStruct.new(x: 1)
  o.x
rescue LoadError => e
  e.message
end
puts lazy
puts defined?(OpenStruct) ? "visible after load" : "invisible"
__END__
1
visible after load
