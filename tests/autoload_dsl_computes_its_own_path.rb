require_relative "autoload_dsl_computes_its_own_path/mylib"

puts MyLib::Model.hello
puts MyLib::Model.name
puts defined?(MyLib::Model)
