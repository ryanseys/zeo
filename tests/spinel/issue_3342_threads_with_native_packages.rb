require "json"
require "stringio"

results = []
th = Thread.new do
  io = StringIO.new
  io.write(JSON.generate({ "a" => 1 }))
  results << io.string
end
th.join
p results
