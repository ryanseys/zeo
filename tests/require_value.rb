# What a `require` answers, and where one may stand. Each line pins CRuby's
# answer: true when this require is what loaded the library, false when it
# was already there -- including `require "rational"`/"complex", the default
# gems folded into core, which answer false. (Imported from spinel.)
r1 = require 'stringio'
p r1
p r1.class
p require('stringio')
p require('set')
p require('rational')
p require('complex')
p require('thread')

# a value in condition position, and the modifier forms
if require 'strscan'
  puts "loaded strscan"
else
  puts "strscan was already there"
end
puts "not reloaded" unless require 'strscan'
p((require('pathname') ? "y" : "n"))
p (require 'stringio' rescue nil)

# indented, and inside a method body: the library still loads, at the top
if 1 > 0
  require 'forwardable'
end

def in_body
  require 'digest'
  "body ok"
end
p in_body

# the libraries all actually work
p StringIO.new("hi").read
p StringScanner.new("ab").scan(/a/)
p Pathname.new("/tmp").to_s
p Set.new([1, 2, 2]).size
p Rational(1, 2) + Rational(1, 3)
p Complex(1, 2) * Complex(3, 4)

# the word inside a string or a heredoc is not a require
src = <<~RB
  require 'no_such_library_at_all'
RB
p src.strip.length
p "require 'also_not_real'".length
# require 'not_this_one_either'
