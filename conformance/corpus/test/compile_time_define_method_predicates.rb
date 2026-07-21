class CompileTimePostState
  attributes :state

  def initialize(state)
    @state = state
  end

  %i[draft published].each do |state|
    define_method("#{state}?") { @state == state }
  end
end

post = CompileTimePostState.new(:draft)
puts post.draft?
puts post.published?
post.state = :published
puts post.draft?
puts post.published?
