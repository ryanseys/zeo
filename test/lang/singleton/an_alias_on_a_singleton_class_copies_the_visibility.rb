# `alias_method` copies the method ENTRY, so the alias carries the source's
# visibility -- on a singleton class exactly as on a plain one.
class C
  class << self
    private

    def hidden = :h
  end
end

C.singleton_class.alias_method(:secret, :hidden)
p C.singleton_class.private_instance_methods(false).sort
begin
  C.secret
rescue NoMethodError => e
  puts "call: #{e.message}"
end
p C.send(:secret)

# A public source stays public, whatever the alias name carried before.
class C
  def self.loud = :l
end
C.singleton_class.alias_method(:shout, :loud)
p C.shout
p C.singleton_class.private_instance_methods(false).sort

# A per-object singleton, and a plain class, take the same copy.
subject = Object.new
def subject.speak = :s
subject.singleton_class.send(:private, :speak)
subject.singleton_class.alias_method(:whisper, :speak)
p subject.singleton_class.private_instance_methods(false).sort

class Plain
  def h = :h
  private :h
end
Plain.alias_method(:s, :h)
p Plain.private_instance_methods(false).sort
__END__
[:hidden, :secret]
call: private method 'secret' called for class C
:h
:l
[:hidden, :secret]
[:speak, :whisper]
[:h, :s]
