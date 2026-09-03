module Wrap
  def detailed_message(highlight: true, **)
    "[" + super + "]"
  end
end

NoMethodError.prepend(Wrap)

begin
  "abc".lenght
rescue NoMethodError => e
  p e.detailed_message(highlight: false)
end
__END__
"[undefined method 'lenght' for an instance of String (NoMethodError)]"
