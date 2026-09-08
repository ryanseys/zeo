# A `class` statement CREATES its constant where it stands, so a read above
# it is a NameError -- the same answer `defined?` gives there.
begin
  Missing.new
rescue NameError => e
  puts "read: #{e.message}"
end
p defined?(Missing)

class Missing
  def initialize = @made = true
end

p Missing.new.class
p defined?(Missing)

# A class body runs at its statement's position, so a read inside one is
# placed the same way.
begin
  module Holder
    Later.name
  end
rescue NameError => e
  puts "in a body: #{e.message}"
end

class Later; end
p Later.name

# A method body runs when it is CALLED, so a read inside one written above
# the class is fine.
def name_of_soon = Soon.name
class Soon; end
p name_of_soon

# The class is still there for everything below it, however deep.
p [Missing, Later, Soon].map(&:name)
__END__
read: uninitialized constant Missing
nil
Missing
"constant"
in a body: uninitialized constant Holder::Later
"Later"
"Soon"
["Missing", "Later", "Soon"]
