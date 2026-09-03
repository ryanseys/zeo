# A require-gated builtin's constant does not exist until its feature is
# required -- so a program that never requires `json` is free to define its
# own `class JSON`, exactly as CRuby is.
#
# zeo's reopen detection saw the dormant slot (deliberately -- a same-kind
# reopen attaches to it) and rejected the kind mismatch with a TypeError
# CRuby never raises: "JSON is not a class". A hundred ledger gems defined
# some builtin's name without its require.
class JSON
  def self.parse(s)
    "my own #{s}"
  end
end

puts JSON.parse("parser")
puts JSON.instance_of?(Class)
__END__
my own parser
true
