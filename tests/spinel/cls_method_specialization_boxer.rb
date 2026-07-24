class Base
  def self.from_row(row)
    instance = new
    if row.is_a?(CommentRow)
      instance.commenter = row.commenter
    elsif row.is_a?(ArticleRow)
      instance.title = row.title
    end
    instance
  end

  def []=(field, value)
    case field
    when :commenter then @commenter = value
    when :title then @title = value
    when :body then @body = value
    end
  end
end

class Comment < Base
  attr_accessor :commenter
  attr_accessor :body
end

class Article < Base
  attr_accessor :title
end

class CommentRow
  attr_reader :commenter
  def initialize(c); @commenter = c; end
end

class ArticleRow
  attr_reader :title
  def initialize(t); @title = t; end
end

seed = Comment.new
seed[:body] = 42
seed[:commenter] = "x"

c = Comment.from_row(CommentRow.new("alice"))
a = Article.from_row(ArticleRow.new("title!"))
puts c.commenter
puts a.title
