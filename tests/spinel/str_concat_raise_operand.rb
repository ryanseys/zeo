# A String-returning method unresolved on a poly value, used as a String#+
# operand, must raise NoMethodError at run time -- not fail C compilation with a
# sp_RbVal-into-const-char* mismatch (#2457 Family 2).
class Note
  def initialize(t); @created_at = t; end
  def created_at; @created_at; end
end
def show(note)
  "at " + note.created_at.strftime("%Y")
end
begin
  show(Note.new(nil))
  p :no_raise
rescue NoMethodError
  p :raised
end
# the ordinary path still works
p("a" + "b")
