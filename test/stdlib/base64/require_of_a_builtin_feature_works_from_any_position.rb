# A `require` of a natively-provided feature is a compile-time act, so it
# works from any position -- and it has CRuby's real return value, true the
# first time and false thereafter (`load.c:1413`).

p(require "digest")
p(require "digest")
if 1 > 0
  require "json"
end
def load_it
  require "set"
  "ok"
end
p load_it
require "base64" if false
require "zlib" rescue nil
puts "done"
__END__
true
false
"ok"
done
