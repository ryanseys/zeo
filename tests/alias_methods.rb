# Both the `alias` keyword and `alias_method` create a second name for an
# existing method, snapshotting its current body. The source method can be
# defined in the same class or inherited from an ancestor.
class Animal
  def speak(sound)
    "the animal says #{sound}"
  end
  alias vocalize speak            # keyword form, same-class source
  alias_method :emit, :speak      # method form, same-class source
end

class Dog < Animal
  alias_method :woof, :speak      # source is INHERITED from Animal
  alias bark speak                # keyword form, inherited source
end

a = Animal.new
puts a.speak("moo")               # the animal says moo
puts a.vocalize("baa")            # the animal says baa
puts a.emit("cluck")              # the animal says cluck

d = Dog.new
puts d.woof("woof")               # the animal says woof
puts d.bark("bark")               # the animal says bark

# Aliases resolve through several inheritance levels, and an alias can even
# alias another alias.
class Puppy < Dog
  alias yip woof                  # woof is Dog's alias of Animal#speak
end
puts Puppy.new.yip("yip")         # the animal says yip
