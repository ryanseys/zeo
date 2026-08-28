require "json"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { require "json/add/range" }
show { JSON.parse('{"json_class":"Range","a":[1,3,false]}', create_additions: true) }
show { (1..3).to_json }
show { [defined?(JSON::Fragment), defined?(JSON::GeneratorError), defined?(JSON::GenericObject)] }
show { JSON.generate({ "pre" => JSON::Fragment.new("[1,2]") }) }
