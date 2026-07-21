class Holder
  def initialize
    @text = nil
  end

  def attach
    @text = "hi"
  end

  def active?
    return false if @text && @text.length == 2

    true
  end
end

h = Holder.new
puts h.active?
h.attach
puts h.active?
