# #700 shape A: when a poly-widened setter receives an attr_reader
# value from an int-typed LV (where codegen's
# compile_int_class_fallback_expr picks the first class with the
# attr_reader to emit `((sp_C *)lv)->iv_X`), the boxer dispatch
# was picking sp_box_int on a `const char *` value.

class Comment
  attr_accessor :commenter

  def []=(field, value)
    case field
    when :commenter then @commenter = value
    when :other then @other = value
    end
  end
end

class CommentRow
  attr_reader :commenter
  def initialize(c); @commenter = c; end
end

# Widen Comment's @commenter to sp_RbVal via case-narrow union with int.
seed = Comment.new
seed[:other] = 42
seed[:commenter] = "x"

def update_from_row(c, p)
  c.commenter = p.commenter
end

c = Comment.new
row = CommentRow.new("alice")
update_from_row(c, row)
puts c.commenter
