require "ostruct" if RUBY_VERSION
def make_struct
  require "ostruct"
  OpenStruct.new(a: 1, b: 2).b
end
puts make_struct

begin
  require "set"
rescue LoadError
  abort "no set"
end
puts Set.new([1, 1, 2, 3]).size

def with_json
  begin
    require "json"
  rescue LoadError
    return "no json"
  end
  JSON.generate({ "k" => 1 })
end
puts with_json
__END__
2
3
{"k":1}
