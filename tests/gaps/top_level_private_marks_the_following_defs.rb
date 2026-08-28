# GAP: `private` at the top level raises `NoMethodError: undefined method
# 'private' for main`.
#
# `main` is an Object, `Module#private` is available there, and ruby uses it:
# a script that wants some of its top-level helpers hidden writes a bare
# `private` or `private :name`. Under zeo neither form exists.
#
# It changes little in practice -- every top-level `def` is ALREADY a private
# instance method of Object -- but the call is common enough in scripts that
# raising on it is worse than the no-op it nearly is. `public :name` at the
# top level is the form that actually does something, and it is missing too.
#
# Found while writing tests/a_universal_def_answers_the_same_on_every_receiver.rb,
# which needed the marker and turned out not to.
def helper
  "helper"
end

private :helper
puts "private with a name: ok"

private
puts "bare private: ok"

def public_one
  "public"
end
public :public_one
puts "public with a name: ok"
puts self.public_one
