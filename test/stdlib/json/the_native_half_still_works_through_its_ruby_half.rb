# Loading the Ruby half must not cost the native half: `require "strscan"`
# resolves `crates/zeo-rt/ext/strscan/lib/strscan.rb`, which pulls the
# native half in with `require "strscan.so"` -- CRuby's loader idiom.

require "strscan"
require "json"
s = StringScanner.new("hello world")
p s.scan(/\w+/)
p s.rest
p JSON.dump({ "a" => 1 })
__END__
"hello"
" world"
"{\"a\":1}"
