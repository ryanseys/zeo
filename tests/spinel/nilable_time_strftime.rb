# A nilable Time (`created_at : Time?`) is held as a poly value, so calling
# strftime on it used to lower to the unresolved-call raise even for a real
# Time. The poly dispatch now gives strftime a Time arm: a genuine Time
# formats, nil (or any other class) raises NoMethodError -- matching CRuby.
# Regression for issue #2457 (the family2 nilable value-method dispatch gap).
class Note
  def initialize(t); @t = t; end
  def created_at; @t; end          # @t is Time-or-nil => Time?
end
def raw(s); s; end
def fmt(a, b); "#{a}=#{b}"; end

t = Note.new(Time.at(0).utc)
Note.new(nil)                       # this call makes created_at nilable

# real Time formats through several result slots
io = String.new
io << t.created_at.strftime("%Y-%m-%d")
puts io
puts raw(t.created_at.strftime("%Y-%m-%d"))
puts fmt("day", t.created_at.strftime("%Y-%m-%d"))
puts "at #{t.created_at.strftime("%H:%M:%S")}"

# nil raises NoMethodError
n = Note.new(nil)
begin
  puts n.created_at.strftime("%F")
rescue NoMethodError
  puts "raised"
end
