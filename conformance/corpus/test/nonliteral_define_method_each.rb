class BadDefineMethodEach
  names = [:one, :two]

  names.each do |name|
    define_method("#{name}") { 1 }
  end
end
