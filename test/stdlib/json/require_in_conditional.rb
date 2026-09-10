# A `require` works wherever it is written: indented inside an `if`, inside a
# method body where it runs when the method does, and in value position where
# it answers whether it did the loading.

if 1 > 0
  require "set"
end

def helper
  require "json"
  "ok"
end

x = require("set")
puts helper
puts "done"
__END__
ok
done
