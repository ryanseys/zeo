# `Regexp#===` against an instance of a String SUBCLASS.
#
# CRuby's `rb_reg_eqq` takes any String, and a subclass instance is one. zeo
# matched on the String representation alone, so a subclass reached the
# "not a string" arm and answered false -- while `match?`, `=~` and `String#=~`
# on the very same object all answered true.
#
# bcrypt is where it showed: `BCrypt::Password` is a String subclass, and its
# `valid_hash?` is a bare `/.../ === h`, so every hash the gem had just
# produced was rejected as invalid.

class Tag < String; end
class Quiet < String
  def to_s = "not this"
end

tag = Tag.new("abc-123")
re = /\A[a-z]+-\d+\z/

puts "is_a String        : #{tag.is_a?(String)}"
puts "regexp ===         : #{(re === tag).inspect}"
puts "regexp === to_s    : #{(re === tag.to_s).inspect}"
puts "regexp match?      : #{re.match?(tag).inspect}"
puts "regexp =~          : #{(re =~ tag).inspect}"
puts "string =~          : #{(tag =~ re).inspect}"
puts "no match           : #{(/\Azzz/ === tag).inspect}"

# `===` reads the string itself, never `to_s` -- a subclass that overrides
# `to_s` still matches on its own characters.
quiet = Quiet.new("abc-123")
puts "ignores to_s       : #{(re === quiet).inspect}"

# `case` takes the same path.
answer = case tag
         when /\A\d/ then "digits"
         when re then "tag"
         else "none"
         end
puts "case when          : #{answer}"

# A match sets `$~` for a subclass the way it does for a String.
re === tag
puts "last match         : #{Regexp.last_match(0).inspect}"

# Everything that is NOT string-like still answers false and clears `$~`.
puts "against an Integer : #{(re === 42).inspect}"
puts "cleared            : #{Regexp.last_match.inspect}"
puts "against a Symbol   : #{(/\Aab/ === :abc).inspect}"
