# An empty-array local (`x = []`) that is then filled with objects via
# `<<` and passed as a call argument must widen the callee's parameter
# (and any ivar it is stored in) to the real element type. Previously
# the class-body call-type pass typed such a local from its `[]` seed
# alone (int_array) and ignored the pushes, so the parameter and ivar
# became int_array. A real object array was then stored and later read
# back as an IntArray, making `.size` / `.length` return garbage.
#
# Mirrors the roundhouse "preload" pattern: a controller groups child
# records into a fresh array (built inside nested `each` blocks, off
# another empty-array local) and hands it to a model's `_preload`,
# which stashes it in an ivar that an accessor reads back.
class Comment
  def initialize(id, aid); @id = id; @aid = aid; end
  def id; @id; end
  def article_id; @aid; end
  def touch(x); @id = x; end          # self-mutating -> heap class
end

class Article
  def initialize(id)
    @id = id
    @cache = []
    @loaded = false
  end
  def id; @id; end
  def _preload(list)
    @cache = list
    @loaded = true
  end
  def comments
    return @cache if @loaded
    fresh = []
    fresh << Comment.new(0, @id)
    fresh
  end
end

class Controller
  def index(articles)
    loaded = []                       # empty-array local #1
    loaded << Comment.new(10, 1)
    loaded << Comment.new(20, 1)
    loaded << Comment.new(30, 2)
    articles.each { |a|
      group = []                      # empty-array local #2 (transitive)
      loaded.each { |r| group << r if r.article_id == a.id }
      a._preload(group) }
  end
end

articles = []
articles << Article.new(1)
articles << Article.new(2)
Controller.new.index(articles)

articles.each { |a| puts a.id.to_s + ": " + a.comments.length.to_s }
