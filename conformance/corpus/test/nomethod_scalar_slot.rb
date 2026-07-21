# An unresolved method on a poly receiver whose call type is a concrete scalar
# must yield that scalar (a raising comma-expr), not the sp_RbVal raise token,
# so it fits a typed slot rather than failing the C compile (#2451). The
# raise still fires at runtime (CRuby's NoMethodError).
class CommentRow
  def created_at; "2020-01-01"; end
end
class StoryRow
  def created_at; "2021-02-02"; end
end
class Wrap
  def initialize(note); @note = note; end
  def render
    s = @note.created_at.strftime("%Y")
    s.to_s
  end
end
begin
  puts Wrap.new(CommentRow.new).render
rescue NoMethodError
  puts "NoMethodError"
end
